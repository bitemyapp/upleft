//! Port of SwiftMath's `Tests/SwiftMathTests/MathImageTests.swift` (as
//! vendored by Downright). Every Swift test function and assertion is kept, in
//! the same order, with the file's helpers (`SwiftMathImageResult`,
//! `NSImage.pngData()`, `Latex.samples`); the dispatch queue and group are
//! `swift_dispatch`.
//!
//! As in Swift, the images are written as PNGs to the temporary directory
//! (`image-test.png`, `image-<case>.png`) for manual inspection.

mod swift_dispatch;

use std::cell::Cell;
use std::sync::Arc;

use objc2::AllocAnyThread;
use objc2::rc::Retained;
use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSColor, NSImage};
use objc2_core_foundation::CGFloat;
use objc2_foundation::{NSData, NSDictionary, NSString};
use swift_dispatch::{
    WorkItem, dispatch_group_wait, random_cgfloat, random_element, random_math_font,
};
use upleft_math::math_bundle::math_font::MathFont;
use upleft_math::math_bundle::math_image::{LayoutInfo, MathImage};
use upleft_math::math_render::mt_math_image::MTMathImage;
use upleft_math::math_render::mt_math_list_builder::MTParseError;
use upleft_math::math_render::mt_math_ui_label::{MTMathUILabelMode, MTTextAlignment};

/// `NSTemporaryDirectory()`.
fn temporary_directory() -> std::path::PathBuf {
    std::env::temp_dir()
}

fn safe_image(file_name: &str, png_data: &NSData) {
    let image_file_path = temporary_directory().join(format!("image-{file_name}.png"));
    // try? pngData.write(to: imageFileURL, options: [.atomicWrite])
    let _ = png_data.writeToFile_atomically(
        &NSString::from_str(image_file_path.to_str().expect("UTF-8 temporary path")),
        true,
    );
    //print("\(#function) \(imageFileURL.path)")
}

#[test]
fn test_math_image_script() {
    let latex = random_element(LATEX_SAMPLES);
    let mathfont = random_math_font();
    let fontsize = random_cgfloat(24.0, 36.0);
    let result = SwiftMathImageResult::use_math_image(latex, mathfont, fontsize);
    assert!(
        result.error.is_none(),
        "{:?} {latex:?} {mathfont:?} {fontsize}",
        result.error
    );
    assert!(result.image.is_some(), "{latex:?} {mathfont:?} {fontsize}");
    assert!(
        result.layout_info.is_some(),
        "{latex:?} {mathfont:?} {fontsize}"
    );
    if result.error.is_none()
        && let Some(image) = &result.image
        && let Some(image_data) = png_data(image)
    {
        safe_image("test", &image_data);
        let file_url = temporary_directory();
        println!(
            "completed, check {} image-test.png =================",
            file_url.display()
        );
    }
}

#[test]
fn test_sequential_multiple_image_script() {
    // `var latex`, `var mathfont` and `var fontsize` are computed: new values per use.
    for case_number in 0..20 {
        let (latex, mathfont, fontsize) = (
            random_element(LATEX_SAMPLES),
            random_math_font(),
            random_cgfloat(20.0, 40.0),
        );
        let result: SwiftMathImageResult;
        match case_number % 2 {
            0 => {
                result = SwiftMathImageResult::use_math_image(latex, mathfont, fontsize);
                assert!(
                    result.error.is_none(),
                    "{:?} {latex:?} {mathfont:?} {fontsize}",
                    result.error
                );
                assert!(result.image.is_some(), "{latex:?} {mathfont:?} {fontsize}");
                assert!(
                    result.layout_info.is_some(),
                    "{latex:?} {mathfont:?} {fontsize}"
                );
                if result.error.is_none()
                    && let Some(image) = &result.image
                    && let Some(image_data) = png_data(image)
                {
                    safe_image(&format!("{case_number}"), &image_data);
                    //let fileUrl = URL(fileURLWithPath: NSTemporaryDirectory())
                    println!("completed image-{case_number}.png");
                }
            }
            _ => {
                result = SwiftMathImageResult::use_mt_math_image(latex, mathfont, fontsize);
                assert!(
                    result.error.is_none(),
                    "{:?} {latex:?} {mathfont:?} {fontsize}",
                    result.error
                );
                assert!(result.image.is_some(), "{latex:?} {mathfont:?} {fontsize}");
                if result.error.is_none()
                    && let Some(image) = &result.image
                    && let Some(image_data) = png_data(image)
                {
                    safe_image(&format!("{case_number}"), &image_data);
                    //let fileUrl = URL(fileURLWithPath: NSTemporaryDirectory())
                    println!("completed image-{case_number}.png");
                }
            }
        }
    }
    println!("check: {} ==", temporary_directory().display());
}

const TOTAL_CASES: usize = 20;

#[test]
fn test_concurrent_math_image_script() {
    let test_count = Cell::new(0);
    // `var latex`, `var mathfont` and `var size` are computed: new values per case.
    let mut items = Vec::new();
    for case_number in 0..TOTAL_CASES {
        let (latex, mathfont, size) = (
            random_element(LATEX_SAMPLES),
            random_math_font(),
            random_cgfloat(20.0, 40.0),
        );
        match case_number % 2 {
            0 => items.push(helper_concurrent_math_image(
                case_number,
                latex,
                mathfont,
                size,
                &test_count,
            )),
            _ => items.push(helper_concurrent_mt_math_image(
                case_number,
                latex,
                mathfont,
                size,
                &test_count,
            )),
        }
    }
    dispatch_group_wait(items);
    // executionGroup.notify(queue: .main)
    let file_url = temporary_directory();
    println!(
        "{}/{} completed, check {} ===",
        test_count.get(),
        TOTAL_CASES,
        file_url.display()
    );
    assert_eq!(test_count.get(), TOTAL_CASES);
}

fn helper_concurrent_math_image<'a>(
    count: usize,
    latex: &'static str,
    mathfont: MathFont,
    fontsize: CGFloat,
    test_count: &'a Cell<usize>,
) -> WorkItem<'a> {
    WorkItem::new(
        move || {
            let result = SwiftMathImageResult::use_math_image(latex, mathfont, fontsize);
            assert!(
                result.error.is_none(),
                "{:?} {latex:?} {mathfont:?} {fontsize}",
                result.error
            );
            assert!(result.image.is_some(), "{latex:?} {mathfont:?} {fontsize}");
            assert!(
                result.layout_info.is_some(),
                "{latex:?} {mathfont:?} {fontsize}"
            );
            if result.error.is_none()
                && let Some(image) = &result.image
                && let Some(image_data) = png_data(image)
            {
                safe_image(&format!("{count}"), &image_data);
            }
        },
        move || {
            test_count.set(test_count.get() + 1);
        },
    )
}

fn helper_concurrent_mt_math_image<'a>(
    count: usize,
    latex: &'static str,
    mathfont: MathFont,
    fontsize: CGFloat,
    test_count: &'a Cell<usize>,
) -> WorkItem<'a> {
    WorkItem::new(
        move || {
            let result = SwiftMathImageResult::use_mt_math_image(latex, mathfont, fontsize);
            assert!(
                result.error.is_none(),
                "{:?} {latex:?} {mathfont:?} {fontsize}",
                result.error
            );
            assert!(result.image.is_some(), "{latex:?} {mathfont:?} {fontsize}");
            if result.error.is_none()
                && let Some(image) = &result.image
                && let Some(image_data) = png_data(image)
            {
                safe_image(&format!("{count}"), &image_data);
            }
        },
        move || {
            test_count.set(test_count.get() + 1);
        },
    )
}

struct SwiftMathImageResult {
    error: Option<MTParseError>,
    image: Option<Retained<NSImage>>,
    layout_info: Option<LayoutInfo>,
}

impl SwiftMathImageResult {
    /// `useMTMathImage(latex:font:fontSize:textColor: MTColor.black)`.
    fn use_mt_math_image(latex: &str, font: MathFont, font_size: CGFloat) -> SwiftMathImageResult {
        let text_color = NSColor::blackColor();
        let alignment = MTTextAlignment::Left;
        let mut formatter = MTMathImage::new(
            latex,
            font_size - 1.0,
            text_color,
            MTMathUILabelMode::Text,
            alignment,
        );
        formatter.font = Some(Arc::new(font.mtfont(font_size)));
        let (error, image) = formatter.as_image();
        SwiftMathImageResult {
            error,
            image,
            layout_info: None,
        }
    }

    /// `useMathImage(latex:font:fontSize:textColor: MTColor.black)`.
    fn use_math_image(latex: &str, font: MathFont, font_size: CGFloat) -> SwiftMathImageResult {
        let text_color = NSColor::blackColor();
        let alignment = MTTextAlignment::Left;
        let mut formatter = MathImage::new(
            latex,
            font_size - 1.0,
            text_color,
            MTMathUILabelMode::Text,
            alignment,
        );
        formatter.font = font;
        let (error, image, layout_info) = formatter.as_image();
        SwiftMathImageResult {
            error,
            image,
            layout_info,
        }
    }
}

/// `NSImage.pngData()`: `tiffRepresentation?.bitmap?.png`, where `bitmap` is
/// `NSBitmapImageRep(data:)` and `png` is `representation(using: .png, properties: [:])`.
fn png_data(image: &NSImage) -> Option<Retained<NSData>> {
    let tiff = image.TIFFRepresentation()?;
    let bitmap = NSBitmapImageRep::initWithData(NSBitmapImageRep::alloc(), &tiff)?;
    unsafe {
        bitmap.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
    }
}

/// `Latex.samples`. The multi-line Swift literals keep four spaces of
/// indentation on each line (their closing delimiter is indented 8 columns,
/// the content 12).
const LATEX_SAMPLES: &[&str] = &[
    r"(a_1 + a_2)^2 = a_1^2 + 2a_1a_2 + a_2^2",
    r"x = \frac{-b \pm \sqrt{b^2-4ac}}{2a}",
    r"\sigma = \sqrt{\frac{1}{N}\sum_{i=1}^N (x_i - \mu)^2}",
    r"\neg(P\land Q) \iff (\neg P)\lor(\neg Q)",
    r"\cos(\theta + \varphi) = \cos(\theta)\cos(\varphi) - \sin(\theta)\sin(\varphi)",
    r"\lim_{x\to\infty}\left(1 + \frac{k}{x}\right)^x = e^k",
    r"f(x) = \int\limits_{-\infty}^\infty\hat f(\xi)\,e^{2 \pi i \xi x}\,\mathrm{d}\xi",
    r"{n \brace k} = \frac{1}{k!}\sum_{j=0}^k (-1)^{k-j}\binom{k}{j}(k-j)^n",
    r"\int_{-\infty}^{\infty} \! e^{-x^2} dx = \sqrt{\pi}",
    r"\frac{1}{n}\sum_{i=1}^{n}x_i \geq \sqrt[n]{\prod_{i=1}^{n}x_i}",
    r"\left(\sum_{k=1}^n a_k b_k \right)^2 \le \left(\sum_{k=1}^n a_k^2\right)\left(\sum_{k=1}^n b_k^2\right)",
    r"\left( \sum_{k=1}^n a_k b_k \right)^2 \leq \left( \sum_{k=1}^n a_k^2 \right) \left( \sum_{k=1}^n b_k^2 \right)",
    r"i\hbar\frac{\partial}{\partial t}\mathbf\Psi(\mathbf{x},t) = -\frac{\hbar}{2m}\nabla^2\mathbf\Psi(\mathbf{x},t) + V(\mathbf{x})\mathbf\Psi(\mathbf{x},t)",
    r#"    \begin{gather}
    \dot{x} = \sigma(y-x) \\
    \dot{y} = \rho x - y - xz \\
    \dot{z} = -\beta z + xy"
    \end{gather}"#,
    r"    \vec \bf V_1 \times \vec \bf V_2 =  \begin{vmatrix}
    \hat \imath &\hat \jmath &\hat k \\
    \frac{\partial X}{\partial u} & \frac{\partial Y}{\partial u} & 0 \\
    \frac{\partial X}{\partial v} & \frac{\partial Y}{\partial v} & 0
    \end{vmatrix}",
    r"    \begin{eqalign}
    \nabla \cdot \vec{\bf E} & = \frac {\rho} {\varepsilon_0} \\
    \nabla \cdot \vec{\bf B} & = 0 \\
    \nabla \times \vec{\bf E} &= - \frac{\partial\vec{\bf B}}{\partial t} \\
    \nabla \times \vec{\bf B} & = \mu_0\vec{\bf J} + \mu_0\varepsilon_0 \frac{\partial\vec{\bf E}}{\partial t}
    \end{eqalign}",
    r"\log_b(x) = \frac{\log_a(x)}{\log_a(b)}",
    r"    \begin{pmatrix}
    a & b\\ c & d
    \end{pmatrix}
    \begin{pmatrix}
    \alpha & \beta \\ \gamma & \delta
    \end{pmatrix} =
    \begin{pmatrix}
    a\alpha + b\gamma & a\beta + b \delta \\
    c\alpha + d\gamma & c\beta + d \delta
    \end{pmatrix}",
    r"    \frak Q(\lambda,\hat{\lambda}) =
    -\frac{1}{2} \mathbb P(O \mid \lambda ) \sum_s \sum_m \sum_t \gamma_m^{(s)} (t) +\\
    \quad \left( \log(2 \pi ) + \log \left| \cal C_m^{(s)} \right| +
    \left( o_t - \hat{\mu}_m^{(s)} \right) ^T \cal C_m^{(s)-1} \right)",
];
