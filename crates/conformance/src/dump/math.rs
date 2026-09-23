//! Mirrors `oracle/Sources/downright-oracle/MathDump.swift`.
//!
//!   upleft-oracle math      <file.tex> <out.png>
//!   upleft-oracle math-tree <file.tex> <out.json>
//!
//! A `.tex` input's first line is `inline` or `display`; the rest is the
//! LaTeX. Both use Paper Light in the light appearance: `inline` is
//! `InlineMathDisplay`'s call (`mathPointSize`, no padding), `display` is
//! `MathFragment`'s (`mathPointSize * 1.12`, 8pt padding).

use std::path::Path;
use std::ptr::NonNull;

use objc2::AnyThread;
use objc2::rc::Retained;
use objc2_app_kit::{
    NSBitmapImageFileType, NSBitmapImageRep, NSColor, NSColorSpace, NSColorType, NSFont,
    NSFontDescriptorSystemDesignSerif, NSFontWeightRegular, NSGraphicsContext, NSImage,
};
use objc2_core_foundation::{
    CFArray, CFDictionary, CFRange, CFRetained, CGFloat, CGPoint, CGRect, CGSize,
};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGColor, CGColorSpace, CGContext, CGGlyph, CGImage, CGImageAlphaInfo,
    kCGColorSpaceSRGB,
};
use objc2_core_text::{
    CTFont, CTLine, CTRun, kCTFontAttributeName, kCTForegroundColorAttributeName,
};
use objc2_foundation::{NSAttributedString, NSDictionary, NSNumber, NSRange};
use serde_json::Value;
use upleft_math::downright::math_renderer::{MathRenderer, trimming_whitespaces_and_newlines};
use upleft_math::math_render::mt_math_image::MTMathImage;
use upleft_math::math_render::mt_math_list::{
    AtomKind, MTColumnAlignment, MTMathAtom, MTMathListRef,
};
use upleft_math::math_render::mt_math_list_builder::{MTMathListBuilder, MTParseError};
use upleft_math::math_render::mt_math_list_display::{DisplayKind, MTDisplay};
use upleft_math::math_render::mt_math_ui_label::{MTMathUILabelMode, MTTextAlignment};

use super::Failure;
use super::json::{Object, double, range, write};

pub(crate) struct Input {
    pub(crate) display: bool,
    pub(crate) latex: String,
}

pub(crate) fn read(path: &Path) -> Result<Input, Failure> {
    let bytes = std::fs::read(path)?;
    let newline = bytes
        .iter()
        .position(|&b| b == b'\n')
        .unwrap_or(bytes.len());
    let style = String::from_utf8_lossy(&bytes[..newline]).into_owned();
    let rest = if newline < bytes.len() {
        &bytes[newline + 1..]
    } else {
        &[][..]
    };
    let latex = String::from_utf8_lossy(rest).into_owned();
    match style.as_str() {
        "inline" => Ok(Input {
            display: false,
            latex,
        }),
        "display" => Ok(Input {
            display: true,
            latex,
        }),
        other => Err(Failure::Error(format!(
            "a .tex input starts with `inline` or `display`, not {other}"
        ))),
    }
}

/// Paper Light, light appearance: `StyleSheet.mathPointSize` and `.text`.
/// The theme values come from the theme file Downright ships; the arithmetic
/// is `StyleSheet.systemFont(preset:size:weight:)`, `mathPointSize(body:typography:)`
/// and `ColorResolver.resolve`.
pub(crate) fn parameters() -> Result<(CGFloat, Retained<NSColor>), Failure> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../vendor/downright/Sources/MarkdownRender/Themes/paper-light.json");
    let theme: Value = serde_json::from_str(&std::fs::read_to_string(&path)?)
        .map_err(|error| Failure::Error(format!("{}: {error}", path.display())))?;
    let typography = &theme["typography"];
    let body_size = typography["bodySize"].as_f64().unwrap_or(16.0);
    let math_scale = typography["mathScale"].as_f64().unwrap_or(1.0);
    let reading = typography["preset"].as_str() == Some("reading");

    let base = NSFont::systemFontOfSize_weight(body_size, unsafe { NSFontWeightRegular });
    let body = if reading {
        base.fontDescriptor()
            .fontDescriptorWithDesign(unsafe { NSFontDescriptorSystemDesignSerif })
            .and_then(|descriptor| NSFont::fontWithDescriptor_size(&descriptor, body_size))
            .unwrap_or(base)
    } else {
        base
    };
    let optical = body.xHeight() / 0.431;
    let clamped = swift_min(swift_max(optical, body_size * 0.90), body_size * 1.10);
    let point_size = clamped * math_scale;

    let hex = theme["palette"]["text"]
        .as_str()
        .unwrap_or("#000000")
        .trim_start_matches('#');
    let v =
        u64::from_str_radix(hex, 16).map_err(|_| Failure::Error(format!("bad colour {hex}")))?;
    let has_alpha = hex.len() == 8;
    let r = ((v >> if has_alpha { 24 } else { 16 }) & 0xFF) as CGFloat / 255.0;
    let g = ((v >> if has_alpha { 16 } else { 8 }) & 0xFF) as CGFloat / 255.0;
    let b = ((v >> if has_alpha { 8 } else { 0 }) & 0xFF) as CGFloat / 255.0;
    let a = if has_alpha {
        (v & 0xFF) as CGFloat / 255.0
    } else {
        1.0
    };
    let color = NSColor::colorWithSRGBRed_green_blue_alpha(r, g, b, a);
    let color = color
        .colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())
        .unwrap_or(color);
    Ok((point_size, color))
}

fn swift_max(x: f64, y: f64) -> f64 {
    if y >= x { y } else { x }
}

fn swift_min(x: f64, y: f64) -> f64 {
    if y < x { y } else { x }
}

fn request(input: &Input) -> Result<(CGFloat, Retained<NSColor>, CGFloat), Failure> {
    let (base, color) = parameters()?;
    Ok(if input.display {
        (base * 1.12, color, 8.0)
    } else {
        (base, color, 0.0)
    })
}

// MARK: - Image

pub fn image(input: &Path, output: &Path) -> Result<(), Failure> {
    let input = read(input)?;
    let (point_size, color, padding) = request(&input)?;
    let image = MathRenderer::image(&input.latex, input.display, point_size, &color, padding);
    let png = match image {
        Some(image) => rasterize(&image)?,
        None => sentinel()?,
    };
    std::fs::write(output, png)?;
    Ok(())
}

fn bitmap_context(width: usize, height: usize) -> Result<CFRetained<CGContext>, Failure> {
    let space = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }))
        .ok_or(Failure::Error("no sRGB".into()))?;
    unsafe {
        CGBitmapContextCreate(
            std::ptr::null_mut(),
            width,
            height,
            8,
            0,
            Some(&space),
            CGImageAlphaInfo::PremultipliedLast.0,
        )
    }
    .ok_or_else(|| Failure::Error("no bitmap context".into()))
}

/// What `drawNSImage` hands to `CGContext.draw`: the image rasterised for a
/// 2x context.
fn rasterize(image: &NSImage) -> Result<Vec<u8>, Failure> {
    let scale: CGFloat = 2.0;
    let size = image.size();
    let width = ((size.width * scale).ceil() as usize).max(1);
    let height = ((size.height * scale).ceil() as usize).max(1);
    let context = bitmap_context(width, height)?;
    CGContext::scale_ctm(Some(&context), scale, scale);
    let mut proposed = CGRect::new(CGPoint::new(0.0, 0.0), size);
    let drawing_context = NSGraphicsContext::graphicsContextWithCGContext_flipped(&context, true);
    let cg_image = unsafe {
        image.CGImageForProposedRect_context_hints(&mut proposed, Some(&drawing_context), None)
    };
    match cg_image {
        Some(cg_image) => png(&cg_image),
        None => sentinel(),
    }
}

fn png(image: &CGImage) -> Result<Vec<u8>, Failure> {
    let rep = NSBitmapImageRep::initWithCGImage(NSBitmapImageRep::alloc(), image);
    let data = unsafe {
        rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
    }
    .ok_or_else(|| Failure::Error("PNG encoding failed".into()))?;
    Ok(data.to_vec())
}

/// A 1×1 opaque magenta pixel: "the renderer returned nil".
fn sentinel() -> Result<Vec<u8>, Failure> {
    let context = bitmap_context(1, 1)?;
    let magenta = CGColor::new_srgb(1.0, 0.0, 1.0, 1.0);
    CGContext::set_fill_color_with_color(Some(&context), Some(&magenta));
    CGContext::fill_rect(
        Some(&context),
        CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(1.0, 1.0)),
    );
    let image = objc2_core_graphics::CGBitmapContextCreateImage(Some(&context))
        .ok_or_else(|| Failure::Error("no image".into()))?;
    png(&image)
}

// MARK: - Display tree

pub fn tree(input: &Path, output: &Path) -> Result<(), Failure> {
    let input = read(input)?;
    let (point_size, color, padding) = request(&input)?;
    write(&tree_json(&input, point_size, &color, padding), output)?;
    Ok(())
}

fn tree_json(
    input: &Input,
    point_size: CGFloat,
    color: &Retained<NSColor>,
    padding: CGFloat,
) -> Value {
    let trimmed = trimming_whitespaces_and_newlines(&input.latex);
    if trimmed.is_empty() {
        return Object::new().with("empty", true).build();
    }
    let font_size = (point_size * 4.0).round() / 4.0;
    let source = MathRenderer::swift_math_source(&trimmed);
    let mut header = Object::new()
        .with("source", source.as_str())
        .with("fontSize", double(font_size))
        .with("padding", double((padding * 2.0).round()))
        .with("style", if input.display { "display" } else { "text" });

    let mut error = None;
    let math_list = MTMathListBuilder::build_from_string_with_error(&source, &mut error);
    let math_list = match (math_list, &error) {
        (Some(list), None) => list,
        _ => return header.with("error", error_json(error.as_ref())).build(),
    };
    header = header
        .with("error", Value::Null)
        .with("list", list_json(Some(&math_list)));

    let mut renderer = MTMathImage::new(
        &source,
        font_size,
        color.clone(),
        if input.display {
            MTMathUILabelMode::Display
        } else {
            MTMathUILabelMode::Text
        },
        if input.display {
            MTTextAlignment::Center
        } else {
            MTTextAlignment::Left
        },
    );
    match renderer.layout() {
        Ok(layout) => header
            .with(
                "size",
                Value::Array(vec![double(layout.size.width), double(layout.size.height)]),
            )
            .with("display", display_json(Some(&layout.display_list)))
            .build(),
        Err(_) => header.with("display", Value::Null).build(),
    }
}

fn error_json(error: Option<&MTParseError>) -> Value {
    match error {
        None => Object::new().with("domain", Value::Null).build(),
        Some(error) => Object::new()
            .with("domain", error.domain())
            .with("code", error.code_value())
            .with("message", error.localized_description())
            .build(),
    }
}

fn ns_range(value: NSRange) -> Value {
    range(value.location, value.length)
}

// MARK: Math list

fn list_json(list: Option<&MTMathListRef>) -> Value {
    match list {
        None => Value::Null,
        Some(list) => Value::Array(
            list.borrow()
                .atoms
                .iter()
                .map(|atom| atom_json(&atom.borrow()))
                .collect(),
        ),
    }
}

fn atom_json(atom: &MTMathAtom) -> Value {
    let mut object = Object::new()
        .with("type", atom.type_.raw_value())
        .with("class", atom.kind.class_name())
        .with("nucleus", atom.nucleus.as_str())
        .with("range", ns_range(atom.index_range))
        .with("fontStyle", atom.font_style as i32)
        .with("superScript", list_json(atom.super_script()))
        .with("subScript", list_json(atom.sub_script()))
        .with("fused", atom.fused_atoms.len());
    match &atom.kind {
        AtomKind::Fraction(fraction) => {
            object = object
                .with("hasRule", fraction.has_rule)
                .with("leftDelimiter", fraction.left_delimiter.as_str())
                .with("rightDelimiter", fraction.right_delimiter.as_str())
                .with("numerator", list_json(fraction.numerator.as_ref()))
                .with("denominator", list_json(fraction.denominator.as_ref()));
        }
        AtomKind::Radical(radical) => {
            object = object
                .with("radicand", list_json(radical.radicand.as_ref()))
                .with("degree", list_json(radical.degree.as_ref()));
        }
        AtomKind::LargeOperator(op) => object = object.with("limits", op.limits),
        AtomKind::Inner(inner) => {
            object = object
                .with("innerList", list_json(inner.inner_list.as_ref()))
                .with(
                    "leftBoundary",
                    inner
                        .left_boundary()
                        .map_or(Value::Null, |b| atom_json(&b.borrow())),
                )
                .with(
                    "rightBoundary",
                    inner
                        .right_boundary()
                        .map_or(Value::Null, |b| atom_json(&b.borrow())),
                );
        }
        AtomKind::OverLine(inner) | AtomKind::UnderLine(inner) | AtomKind::Accent(inner) => {
            object = object.with("innerList", list_json(inner.inner_list.as_ref()));
        }
        AtomKind::Space(space) => object = object.with("space", double(space.space)),
        AtomKind::Style(style) => object = object.with("style", style.style.raw_value()),
        AtomKind::Color(color) | AtomKind::TextColor(color) | AtomKind::Colorbox(color) => {
            object = object
                .with("colorString", color.color_string.as_str())
                .with("innerList", list_json(color.inner_list.as_ref()));
        }
        AtomKind::Table(table) => {
            object = object
                .with("environment", table.environment.as_str())
                .with(
                    "alignments",
                    Value::Array(
                        table
                            .alignments
                            .iter()
                            .map(|alignment| {
                                Value::from(match alignment {
                                    MTColumnAlignment::Left => "left",
                                    MTColumnAlignment::Center => "center",
                                    MTColumnAlignment::Right => "right",
                                })
                            })
                            .collect(),
                    ),
                )
                .with("interColumnSpacing", double(table.inter_column_spacing))
                .with(
                    "interRowAdditionalSpacing",
                    double(table.inter_row_additional_spacing),
                )
                .with(
                    "cells",
                    Value::Array(
                        table
                            .cells
                            .iter()
                            .map(|row| {
                                Value::Array(row.iter().map(|cell| list_json(Some(cell))).collect())
                            })
                            .collect(),
                    ),
                );
        }
        AtomKind::Atom => {}
    }
    object.build()
}

// MARK: Displays

fn point(point: CGPoint) -> Value {
    Value::Array(vec![double(point.x), double(point.y)])
}

fn ct_font_json(font: Option<&CTFont>) -> Value {
    match font {
        None => Value::Null,
        Some(font) => Object::new()
            .with("name", unsafe { font.post_script_name() }.to_string())
            .with("size", double(unsafe { font.size() }))
            .build(),
    }
}

fn cg_color_json(color: &CGColor) -> Value {
    let space = CGColor::color_space(Some(color));
    let name = space
        .as_deref()
        .and_then(|space| CGColorSpace::name(Some(space)))
        .map(|name| name.to_string())
        .unwrap_or_default();
    let count = CGColor::number_of_components(Some(color));
    let components = CGColor::components(Some(color));
    let components: Vec<Value> = if components.is_null() {
        Vec::new()
    } else {
        (0..count)
            .map(|i| double(unsafe { *components.add(i) }))
            .collect()
    };
    Object::new()
        .with("colorSpace", name)
        .with("components", Value::Array(components))
        .build()
}

/// `AttributeDump.colorJSON`.
fn ns_color_json(color: Option<&Retained<NSColor>>) -> Value {
    let Some(color) = color else {
        return Value::Null;
    };
    let color_type = color.r#type();
    if color_type == NSColorType::Catalog {
        return Object::new()
            .with("type", "NSColor")
            .with("colorType", "catalog")
            .with("catalog", color.catalogNameComponent().to_string())
            .with("name", color.colorNameComponent().to_string())
            .build();
    }
    if color_type == NSColorType::ComponentBased {
        let space = color.colorSpace();
        let count = color.numberOfComponents() as usize;
        let mut components = vec![0.0 as CGFloat; count.max(1)];
        unsafe { color.getComponents(NonNull::new(components.as_mut_ptr()).unwrap()) };
        components.truncate(count);
        let name = space
            .localizedName()
            .map(|name| name.to_string())
            .unwrap_or_else(|| format!("{}", space.colorSpaceModel().0));
        return Object::new()
            .with("type", "NSColor")
            .with("colorType", "componentBased")
            .with("colorSpace", name)
            .with(
                "components",
                Value::Array(components.into_iter().map(double).collect()),
            )
            .build();
    }
    Object::new()
        .with("type", "NSColor")
        .with("colorType", "pattern")
        .build()
}

fn number_json(number: &NSNumber) -> Value {
    // Only kern values (doubles) are numbers in a math line's attributes.
    let objc_type = number.objCType();
    let is_float = matches!(unsafe { *objc_type.as_ptr() } as u8, b'f' | b'd');
    if is_float {
        Object::new()
            .with("type", "Double")
            .with("value", double(number.doubleValue()))
            .build()
    } else {
        Object::new()
            .with("type", "Int")
            .with("value", number.integerValue())
            .build()
    }
}

fn attributes_json(string: &NSAttributedString) -> Value {
    let font_key = unsafe { kCTFontAttributeName }.to_string();
    let color_key = unsafe { kCTForegroundColorAttributeName }.to_string();
    let mut runs = Vec::new();
    let length = string.length();
    let mut location = 0;
    while location < length {
        let mut effective = NSRange::new(0, 0);
        let attributes =
            unsafe { string.attributesAtIndex_effectiveRange(location, &mut effective) };
        let (keys, values) = attributes.to_vecs();
        let mut pairs: Vec<(String, Value)> = Vec::new();
        for (key, value) in keys.iter().zip(values.iter()) {
            let key = key.to_string();
            let json = if key == font_key {
                ct_font_json(Some(unsafe { &*(&**value as *const _ as *const CTFont) }))
            } else if key == color_key {
                cg_color_json(unsafe { &*(&**value as *const _ as *const CGColor) })
            } else if let Some(number) = value.downcast_ref::<NSNumber>() {
                number_json(number)
            } else {
                Value::from(format!("{value:?}"))
            };
            pairs.push((key, json));
        }
        pairs.sort_by(|a, b| a.0.cmp(&b.0));
        let mut object = Object::new().with("range", ns_range(effective));
        for (key, value) in pairs {
            object = object.with(&key, value);
        }
        runs.push(object.build());
        location = effective.location + effective.length;
    }
    Value::Array(runs)
}

fn runs_json(line: &CTLine) -> Value {
    let runs: CFRetained<CFArray> = unsafe { line.glyph_runs() };
    let count = runs.count();
    let mut out = Vec::with_capacity(count as usize);
    for index in 0..count {
        let run = unsafe { &*(runs.value_at_index(index) as *const CTRun) };
        let glyph_count = unsafe { run.glyph_count() };
        let mut glyphs = vec![0 as CGGlyph; glyph_count as usize];
        let mut positions = vec![CGPoint::new(0.0, 0.0); glyph_count as usize];
        if glyph_count > 0 {
            unsafe {
                run.glyphs(
                    CFRange::new(0, glyph_count),
                    NonNull::new(glyphs.as_mut_ptr()).unwrap(),
                );
                run.positions(
                    CFRange::new(0, glyph_count),
                    NonNull::new(positions.as_mut_ptr()).unwrap(),
                );
            }
        }
        let string_range = unsafe { run.string_range() };
        let attributes: CFRetained<CFDictionary> = unsafe { run.attributes() };
        let font = unsafe {
            let key = kCTFontAttributeName as *const _ as *const std::ffi::c_void;
            let value = attributes.value(key);
            (!value.is_null()).then(|| &*(value as *const CTFont))
        };
        out.push(
            Object::new()
                .with(
                    "stringRange",
                    Value::Array(vec![
                        string_range.location.into(),
                        string_range.length.into(),
                    ]),
                )
                .with("font", ct_font_json(font))
                .with(
                    "glyphs",
                    Value::Array(glyphs.iter().map(|&glyph| Value::from(glyph)).collect()),
                )
                .with(
                    "positions",
                    Value::Array(positions.into_iter().map(point).collect()),
                )
                .build(),
        );
    }
    Value::Array(out)
}

fn display_json(display: Option<&MTDisplay>) -> Value {
    let Some(display) = display else {
        return Value::Null;
    };
    let mut object = Object::new()
        .with("class", display.class_name())
        .with("position", point(display.position()))
        .with("width", double(display.width()))
        .with("ascent", double(display.ascent()))
        .with("descent", double(display.descent()))
        .with("range", ns_range(display.range))
        .with("hasScript", display.has_script)
        .with("textColor", ns_color_json(display.text_color()))
        .with(
            "localTextColor",
            ns_color_json(display.local_text_color.as_ref()),
        )
        .with(
            "localBackgroundColor",
            ns_color_json(display.local_background_color.as_ref()),
        );
    match &display.kind {
        DisplayKind::CTLine(line) => {
            object = object
                .with("string", line.attributed_string.string().to_string())
                .with("attributes", attributes_json(&line.attributed_string))
                .with("runs", runs_json(&line.line))
                .with(
                    "atoms",
                    Value::Array(
                        line.atoms
                            .iter()
                            .map(|atom| {
                                let atom = atom.borrow();
                                Object::new()
                                    .with("type", atom.type_.raw_value())
                                    .with("nucleus", atom.nucleus.as_str())
                                    .with("range", ns_range(atom.index_range))
                                    .build()
                            })
                            .collect(),
                    ),
                );
        }
        DisplayKind::MathList(list) => {
            object = object
                .with("linePosition", list.type_ as isize)
                .with("index", list.index)
                .with(
                    "subDisplays",
                    Value::Array(
                        list.sub_displays
                            .iter()
                            .map(|d| display_json(Some(d)))
                            .collect(),
                    ),
                );
        }
        DisplayKind::Fraction(fraction) => {
            object = object
                .with("numerator", display_json(fraction.numerator.as_deref()))
                .with("denominator", display_json(fraction.denominator.as_deref()))
                .with("numeratorUp", double(fraction.numerator_up()))
                .with("denominatorDown", double(fraction.denominator_down()))
                .with("linePosition", double(fraction.line_position))
                .with("lineThickness", double(fraction.line_thickness));
        }
        DisplayKind::Radical(radical) => {
            object = object
                .with("radicand", display_json(radical.radicand.as_deref()))
                .with("degree", display_json(radical.degree.as_deref()))
                .with("radicalGlyph", display_json(radical.radical_glyph()))
                .with("radicalShift", double(radical.radical_shift()))
                .with("topKern", double(radical.top_kern))
                .with("lineThickness", double(radical.line_thickness));
        }
        DisplayKind::Glyph(glyph) => {
            object = object
                .with("glyph", glyph.glyph)
                .with(
                    "font",
                    ct_font_json(glyph.font.as_ref().map(|font| font.ct_font())),
                )
                .with("shiftDown", double(glyph.shift_down));
        }
        DisplayKind::GlyphConstruction(construction) => {
            object = object
                .with(
                    "glyphs",
                    Value::Array(
                        construction
                            .glyphs
                            .iter()
                            .map(|&glyph| Value::from(glyph))
                            .collect(),
                    ),
                )
                .with(
                    "positions",
                    Value::Array(construction.positions.iter().copied().map(point).collect()),
                )
                .with(
                    "font",
                    ct_font_json(construction.font.as_ref().map(|font| font.ct_font())),
                )
                .with("shiftDown", double(construction.shift_down));
        }
        DisplayKind::LargeOpLimits(limits) => {
            object = object
                .with("nucleus", display_json(limits.nucleus.as_deref()))
                .with("upperLimit", display_json(limits.upper_limit.as_deref()))
                .with("lowerLimit", display_json(limits.lower_limit.as_deref()))
                .with("limitShift", double(limits.limit_shift))
                .with("upperLimitGap", double(limits.upper_limit_gap()))
                .with("lowerLimitGap", double(limits.lower_limit_gap()))
                .with("extraPadding", double(limits.extra_padding));
        }
        DisplayKind::Line(line) => {
            object = object
                .with("inner", display_json(line.inner.as_deref()))
                .with("lineShiftUp", double(line.line_shift_up))
                .with("lineThickness", double(line.line_thickness));
        }
        DisplayKind::Accent(accent) => {
            object = object
                .with("accentee", display_json(accent.accentee.as_deref()))
                .with("accent", display_json(accent.accent.as_deref()));
        }
    }
    object.build()
}
