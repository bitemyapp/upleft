//! Mirrors `oracle/Sources/downright-oracle/MathBench.swift`:
//!
//!   upleft-oracle bench-math <dir> <out.json>
//!
//! The same stages over the same `.tex` inputs, with drbench's `measure`
//! harness (one warm-up, `MATH_BENCH_RUNS` runs, nearest-rank p50/p95).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use objc2_app_kit::{NSGraphicsContext, NSImage};
use objc2_core_foundation::{CGFloat, CGPoint, CGRect};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGColorSpace, CGContext, CGImageAlphaInfo, kCGColorSpaceSRGB,
};
use serde_json::Value;
use upleft_math::downright::math_renderer::{MathRenderer, trimming_whitespaces_and_newlines};
use upleft_math::math_render::mt_font::MTFont;
use upleft_math::math_render::mt_font_manager::MTFontManager;
use upleft_math::math_render::mt_math_image::MTMathImage;
use upleft_math::math_render::mt_math_list::MTLineStyle;
use upleft_math::math_render::mt_math_list_builder::MTMathListBuilder;
use upleft_math::math_render::mt_math_ui_label::{MTMathUILabelMode, MTTextAlignment};
use upleft_math::math_render::mt_typesetter::MTTypesetter;

use super::Failure;
use super::json::{Object, double, write};
use super::math::{parameters, read};

struct Formula {
    source: String,
    display: bool,
    font_size: CGFloat,
}

fn tex_files(directory: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            tex_files(&path, out);
        } else if path.extension().is_some_and(|extension| extension == "tex") {
            out.push(path);
        }
    }
}

fn formulas(directory: &Path) -> Result<Vec<Formula>, Failure> {
    let (base, _) = parameters("Paper Light", false)?;
    let mut files = Vec::new();
    tex_files(directory, &mut files);
    files.sort();
    let mut out = Vec::with_capacity(files.len());
    for file in files {
        let input = read(&file)?;
        let trimmed = trimming_whitespaces_and_newlines(&input.latex);
        if trimmed.is_empty() {
            continue;
        }
        let point_size = if input.display { base * 1.12 } else { base };
        out.push(Formula {
            source: MathRenderer::swift_math_source(&trimmed),
            display: input.display,
            font_size: (point_size * 4.0).round() / 4.0,
        });
    }
    Ok(out)
}

fn percentile(ascending: &[f64], p: f64) -> f64 {
    let rank = (p * ascending.len() as f64).ceil() as isize;
    ascending[((rank - 1).max(0) as usize).min(ascending.len() - 1)]
}

fn measure(label: &str, runs: usize, mut body: impl FnMut()) -> (String, Value) {
    body();
    let mut samples = Vec::with_capacity(runs);
    for _ in 0..runs {
        let start = Instant::now();
        body();
        samples.push(start.elapsed().as_nanos() as f64 / 1_000_000.0);
    }
    samples.sort_by(f64::total_cmp);
    let (p50, p95, max) = (
        percentile(&samples, 0.50),
        percentile(&samples, 0.95),
        *samples.last().unwrap(),
    );
    println!("  {label:<44}  p50 {p50:8.3} ms   p95 {p95:8.3} ms   max {max:8.3} ms (n={runs})");
    (
        label.to_owned(),
        Object::new()
            .with("p50", double(p50))
            .with("p95", double(p95))
            .with("max", double(max))
            .with("runs", runs)
            .build(),
    )
}

/// `MathDump.rasterize` without the PNG encoding: the bitmap `drawNSImage` draws.
fn rasterize(image: &NSImage) -> Option<usize> {
    let scale: CGFloat = 2.0;
    let size = image.size();
    let width = ((size.width * scale).ceil() as usize).max(1);
    let height = ((size.height * scale).ceil() as usize).max(1);
    let space = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }))?;
    let context = unsafe {
        CGBitmapContextCreate(
            std::ptr::null_mut(),
            width,
            height,
            8,
            0,
            Some(&space),
            CGImageAlphaInfo::PremultipliedLast.0,
        )
    }?;
    CGContext::scale_ctm(Some(&context), scale, scale);
    let mut proposed = CGRect::new(CGPoint::new(0.0, 0.0), size);
    let drawing_context = NSGraphicsContext::graphicsContextWithCGContext_flipped(&context, true);
    let image = unsafe {
        image.CGImageForProposedRect_context_hints(&mut proposed, Some(&drawing_context), None)
    }?;
    Some(objc2_core_graphics::CGImage::width(Some(&image)))
}

pub fn run(input: &Path, output: &Path) -> Result<(), Failure> {
    let formulas = formulas(input)?;
    let (_, color) = parameters("Paper Light", false)?;
    let runs: usize = std::env::var("MATH_BENCH_RUNS")
        .ok()
        .and_then(|runs| runs.parse().ok())
        .unwrap_or(10);
    let mut fonts: HashMap<u64, Arc<MTFont>> = HashMap::new();
    for formula in &formulas {
        fonts.entry(formula.font_size.to_bits()).or_insert_with(|| {
            MTFontManager::font_manager()
                .default_font()
                .unwrap()
                .copy_with_size(formula.font_size)
        });
    }
    let mut sink = 0usize;
    println!("bench-math: {} formulas, {runs} runs", formulas.len());
    let mut results = vec![measure("parse", runs, || {
        for formula in &formulas {
            sink = sink.wrapping_add(
                MTMathListBuilder::build_from_string(&formula.source)
                    .map_or(0, |list| list.borrow().atoms.len()),
            );
        }
    })];
    results.push(measure("parse + typeset", runs, || {
        for formula in &formulas {
            let Some(list) = MTMathListBuilder::build_from_string(&formula.source) else {
                continue;
            };
            let style = if formula.display {
                MTLineStyle::Display
            } else {
                MTLineStyle::Text
            };
            let display = MTTypesetter::create_line_for_math_list(
                Some(&list),
                &fonts[&formula.font_size.to_bits()],
                style,
            );
            sink = sink.wrapping_add(display.map_or(0, |display| display.sub_displays().len()));
        }
    }));
    results.push(measure("parse + typeset + render", runs, || {
        for formula in &formulas {
            let mut renderer = MTMathImage::new(
                &formula.source,
                formula.font_size,
                color.clone(),
                if formula.display {
                    MTMathUILabelMode::Display
                } else {
                    MTMathUILabelMode::Text
                },
                if formula.display {
                    MTTextAlignment::Center
                } else {
                    MTTextAlignment::Left
                },
            );
            let (_, image) = renderer.as_image();
            if let Some(width) = image.as_deref().and_then(rasterize) {
                sink = sink.wrapping_add(width);
            }
        }
    }));
    let mut object = Object::new();
    for (label, value) in results {
        object = object.with(&label, value);
    }
    let object = object
        .with("formulas", formulas.len())
        .with("sink", sink & 1);
    write(&object.build(), output)?;
    Ok(())
}
