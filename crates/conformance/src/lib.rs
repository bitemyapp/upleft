//! Exact comparison of the dumps and captures `downright-oracle` (Swift) and
//! `upleft-oracle` (Rust) write for the same document.
//!
//! JSON is compared structurally: key order and number formatting are
//! irrelevant, values are not. Floating-point numbers must be bit-identical
//! (`-0.0` differs from `0.0`). PNGs are decoded and compared pixel by pixel;
//! any differing channel in any pixel is a failure.

pub mod dump;

use std::fmt;
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use serde_json::Value;

/// One place where the two documents disagree.
#[derive(Debug, Clone, PartialEq)]
pub struct Difference {
    /// JSON-pointer-like path, e.g. `/root/children/3/range/0`.
    pub path: String,
    pub swift: String,
    pub rust: String,
}

impl fmt::Display for Difference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: swift {} ≠ rust {}", self.path, self.swift, self.rust)
    }
}

fn brief(value: &Value) -> String {
    let text = value.to_string();
    if text.chars().count() > 160 {
        let cut: String = text.chars().take(157).collect();
        format!("{cut}…")
    } else {
        text
    }
}

fn numbers_equal(a: &serde_json::Number, b: &serde_json::Number) -> bool {
    match (a.as_i64(), b.as_i64(), a.as_u64(), b.as_u64()) {
        (Some(x), Some(y), _, _) => x == y,
        (_, _, Some(x), Some(y)) => x == y,
        _ => match (a.as_f64(), b.as_f64()) {
            // An integer-valued double prints as `2.0` in Swift and Rust
            // alike, so a float on one side and an integer on the other is a
            // real disagreement about the value's type, not formatting.
            (Some(x), Some(y)) if a.is_f64() && b.is_f64() => x.to_bits() == y.to_bits(),
            _ => false,
        },
    }
}

/// Collects up to `limit` differences between `swift` and `rust`.
pub fn compare_json(swift: &Value, rust: &Value, limit: usize) -> Vec<Difference> {
    let mut out = Vec::new();
    walk(swift, rust, &mut String::new(), &mut out, limit);
    out
}

fn walk(swift: &Value, rust: &Value, path: &mut String, out: &mut Vec<Difference>, limit: usize) {
    if out.len() >= limit {
        return;
    }
    let mismatch = |path: &str, out: &mut Vec<Difference>| {
        out.push(Difference {
            path: if path.is_empty() { "/".into() } else { path.into() },
            swift: brief(swift),
            rust: brief(rust),
        })
    };
    match (swift, rust) {
        (Value::Null, Value::Null) => {}
        (Value::Bool(a), Value::Bool(b)) if a == b => {}
        (Value::String(a), Value::String(b)) if a == b => {}
        (Value::Number(a), Value::Number(b)) if numbers_equal(a, b) => {}
        (Value::Array(a), Value::Array(b)) => {
            let shared = a.len().min(b.len());
            for index in 0..shared {
                let length = path.len();
                path.push('/');
                path.push_str(&index.to_string());
                walk(&a[index], &b[index], path, out, limit);
                path.truncate(length);
                if out.len() >= limit {
                    return;
                }
            }
            if a.len() != b.len() {
                out.push(Difference {
                    path: format!("{path}/length"),
                    swift: a.len().to_string(),
                    rust: b.len().to_string(),
                });
            }
        }
        (Value::Object(a), Value::Object(b)) => {
            for (key, swift_value) in a {
                let length = path.len();
                path.push('/');
                path.push_str(key);
                match b.get(key) {
                    Some(rust_value) => walk(swift_value, rust_value, path, out, limit),
                    None => out.push(Difference {
                        path: path.clone(),
                        swift: brief(swift_value),
                        rust: "(missing)".into(),
                    }),
                }
                path.truncate(length);
                if out.len() >= limit {
                    return;
                }
            }
            for (key, rust_value) in b {
                if !a.contains_key(key) && out.len() < limit {
                    out.push(Difference {
                        path: format!("{path}/{key}"),
                        swift: "(missing)".into(),
                        rust: brief(rust_value),
                    });
                }
            }
        }
        _ => mismatch(path, out),
    }
}

/// A decoded image in 8-bit RGBA, unpremultiplied as PNG stores it.
#[derive(Debug, Clone)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub fn read_png(path: &Path) -> Result<Image, String> {
    let file = File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|error| format!("{}: {error}", path.display()))?;
    let mut buffer = vec![0; reader.output_buffer_size().ok_or("image too large")?];
    let info = reader
        .next_frame(&mut buffer)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    buffer.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgba => buffer,
        png::ColorType::Rgb => buffer.chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => buffer.chunks(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        png::ColorType::Grayscale => buffer.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return Err(format!("{}: indexed PNG after expansion", path.display())),
    };
    Ok(Image { width: info.width, height: info.height, rgba })
}

pub fn write_png(path: &Path, image: &Image) -> Result<(), String> {
    let file = File::create(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut encoder = png::Encoder::new(BufWriter::new(file), image.width, image.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|error| error.to_string())?;
    writer.write_image_data(&image.rgba).map_err(|error| error.to_string())
}

#[derive(Debug, Clone, PartialEq)]
pub enum PixelComparison {
    Identical,
    SizeMismatch { swift: (u32, u32), rust: (u32, u32) },
    Different {
        pixels: usize,
        max_channel_delta: u8,
        /// Bounding box of the differing pixels, in pixels: x, y, width, height.
        bounds: (u32, u32, u32, u32),
    },
}

impl PixelComparison {
    pub fn is_identical(&self) -> bool {
        matches!(self, PixelComparison::Identical)
    }
}

impl fmt::Display for PixelComparison {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PixelComparison::Identical => write!(f, "identical"),
            PixelComparison::SizeMismatch { swift, rust } => {
                write!(f, "size swift {}×{} ≠ rust {}×{}", swift.0, swift.1, rust.0, rust.1)
            }
            PixelComparison::Different { pixels, max_channel_delta, bounds } => write!(
                f,
                "{pixels} pixels differ (max channel Δ {max_channel_delta}) within x {} y {} w {} h {}",
                bounds.0, bounds.1, bounds.2, bounds.3
            ),
        }
    }
}

/// Compares two images exactly. When they differ and `diff` is given, writes
/// a visualisation there: the Swift image dimmed, differing pixels in red.
pub fn compare_images(swift: &Image, rust: &Image, diff: Option<&Path>) -> Result<PixelComparison, String> {
    if (swift.width, swift.height) != (rust.width, rust.height) {
        return Ok(PixelComparison::SizeMismatch {
            swift: (swift.width, swift.height),
            rust: (rust.width, rust.height),
        });
    }
    let mut pixels = 0usize;
    let mut max_delta = 0u8;
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (u32::MAX, u32::MAX, 0u32, 0u32);
    let mut visual = diff.map(|_| Vec::with_capacity(swift.rgba.len()));
    for (index, (a, b)) in swift.rgba.chunks_exact(4).zip(rust.rgba.chunks_exact(4)).enumerate() {
        let delta = a.iter().zip(b).map(|(x, y)| x.abs_diff(*y)).max().unwrap_or(0);
        if delta > 0 {
            pixels += 1;
            max_delta = max_delta.max(delta);
            let x = index as u32 % swift.width;
            let y = index as u32 / swift.width;
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
        if let Some(visual) = visual.as_mut() {
            if delta > 0 {
                visual.extend_from_slice(&[255, 0, 0, 255]);
            } else {
                let grey = |c: u8| 160 + c / 3;
                visual.extend_from_slice(&[grey(a[0]), grey(a[1]), grey(a[2]), 255]);
            }
        }
    }
    if pixels == 0 {
        return Ok(PixelComparison::Identical);
    }
    if let (Some(path), Some(rgba)) = (diff, visual) {
        write_png(path, &Image { width: swift.width, height: swift.height, rgba })?;
    }
    Ok(PixelComparison::Different {
        pixels,
        max_channel_delta: max_delta,
        bounds: (min_x, min_y, max_x - min_x + 1, max_y - min_y + 1),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn doubles_compare_by_bits_and_integers_by_value() {
        assert!(compare_json(&json!({"a": 1.5, "b": 2}), &json!({"b": 2, "a": 1.5}), 10).is_empty());
        let negative_zero: Value = serde_json::from_str("-0.0").unwrap();
        assert_eq!(compare_json(&json!(0.0), &negative_zero, 10).len(), 1);
        assert_eq!(compare_json(&json!(2.0), &json!(2), 10).len(), 1);
    }

    #[test]
    fn reports_paths_and_length_mismatches() {
        let differences = compare_json(&json!({"a": [1, 2, 3]}), &json!({"a": [1, 5]}), 10);
        assert_eq!(differences[0].path, "/a/1");
        assert_eq!(differences[1].path, "/a/length");
    }
}
