//! Port of `Sources/DownrightApp/Export/HTMLExporter.swift`: self-contained
//! HTML export (§9.5), plus `Slugs` (GitHub-compatible heading slugs).
//!
//! "Self-contained" is the whole requirement: styles inlined, images embedded
//! as data URIs, and math and diagrams embedded as PNGs produced by the same
//! native renderers the app draws with (through a [`FragmentImageProvider`]).
//!
//! Every string rule follows the Swift's `String` semantics: `escape` walks
//! Characters (so `<` followed by a combining mark is one Character that is
//! not `<`, and ships unescaped, as in Downright), prefix and suffix tests are
//! Character-wise, and colours go through `NSColor` exactly as the Swift
//! converts them. `NativeFragmentImageProvider`, which Swift declares in
//! `App/DocumentWindowController+Support.swift`, lives here because the
//! window controller is not part of this crate.

use std::path::Path;
use std::sync::Arc;

use objc2::Message;
use objc2::rc::Retained;
use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSColor, NSColorSpace, NSImage};
use objc2_core_foundation::CGFloat;
use objc2_foundation::NSDictionary;
use upleft_core::{BlockContent, InlineKind, InlineSpan, MDBlock, NSRange, ParsedDocument, TableAlignment, TableData};
use upleft_foundation::url::FileUrl;
use upleft_math::downright::math_renderer::MathRenderer;
use upleft_mermaid::downright::mermaid_renderer_bridge;
use upleft_render::render_contracts::{BodyPreset, Theme, ThemeAppearance, ThemeColor};
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_swift_text::{self as swift, CharSet};

use crate::support::find_engine::file_size;

/// Supplies rendered bitmaps for fragments a browser can't draw itself.
/// Injected rather than imported so the exporter stays testable and still
/// works (minus pictures) with no renderer attached.
pub trait FragmentImageProvider {
    fn image_for_math(&self, latex: &str, display: bool, point_size: CGFloat, color: &NSColor)
    -> Option<Retained<NSImage>>;
    fn image_for_mermaid(&self, source: &str, theme: &Theme) -> Option<Retained<NSImage>>;
}

/// `NativeFragmentImageProvider`: bridges export to the renderers so the file
/// carries the same math and diagrams the app draws.
pub struct NativeFragmentImageProvider {
    pub style_sheet: StyleSheet,
}

impl FragmentImageProvider for NativeFragmentImageProvider {
    fn image_for_math(
        &self,
        latex: &str,
        display: bool,
        point_size: CGFloat,
        color: &NSColor,
    ) -> Option<Retained<NSImage>> {
        MathRenderer::image(latex, display, point_size, color, 0.0)
    }

    fn image_for_mermaid(&self, source: &str, _theme: &Theme) -> Option<Retained<NSImage>> {
        mermaid_renderer_bridge::image(source, &self.style_sheet).map(|image| image.ns_image())
    }
}

pub struct HTMLExporter {
    pub document: Arc<ParsedDocument>,
    pub theme: Theme,
    pub title: String,
    pub base_directory: Option<FileUrl>,
    pub image_provider: Option<Box<dyn FragmentImageProvider>>,
    /// Print-oriented stylesheet: a document set for paper, not a screenshot
    /// of the screen theme (§9.5).
    pub for_print: bool,
}

/// `Int(x)` for a finite `CGFloat` (truncation toward zero).
fn swift_int(value: f64) -> i64 {
    value as i64
}

/// `Double.description`.
fn double_description(value: f64) -> String {
    upleft_mermaid::swift::double_description(value)
}

/// `String(format:)` with one `CGFloat`: Foundation formats doubles with the
/// C library in the C locale.
fn format_double(format: &str, value: f64) -> String {
    let format = std::ffi::CString::new(format).expect("no NUL in a format");
    let mut buffer = [0u8; 128];
    // SAFETY: the buffer is large enough for the short formats used here, and
    // snprintf NUL-terminates within `buffer.len()`.
    let written = unsafe { libc::snprintf(buffer.as_mut_ptr().cast(), buffer.len(), format.as_ptr(), value) };
    String::from_utf8_lossy(&buffer[..(written.max(0) as usize).min(buffer.len() - 1)]).into_owned()
}

/// `String(format: "#%02x%02x%02x", r, g, b)` with Swift `Int`s: `%x` reads
/// the low 32 bits of each argument.
fn hex_string(r: i64, g: i64, b: i64) -> String {
    format!("#{:02x}{:02x}{:02x}", r as u32, g as u32, b as u32)
}

/// `Int((component * 255).rounded())`.
fn component_byte(component: CGFloat) -> i64 {
    swift_int((component * 255.0).round())
}

fn srgb() -> Retained<NSColorSpace> {
    NSColorSpace::sRGBColorSpace()
}

/// `hex(_ color: ThemeColor)`: resolved, converted to sRGB (else
/// `labelColor`), each component rounded to a byte.
fn hex(color: &ThemeColor) -> String {
    let resolved = color.resolved().colorUsingColorSpace(&srgb()).unwrap_or_else(NSColor::labelColor);
    hex_components(&resolved)
}

/// `hex(_ color: NSColor)`: converted to sRGB when possible.
fn hex_color(color: &NSColor) -> String {
    let resolved = color.colorUsingColorSpace(&srgb());
    hex_components(resolved.as_deref().unwrap_or(color))
}

fn hex_components(color: &NSColor) -> String {
    hex_string(
        component_byte(color.redComponent()),
        component_byte(color.greenComponent()),
        component_byte(color.blueComponent()),
    )
}

/// `blend(_:_:amount:)`: a straight sRGB interpolation, or `first` when
/// either colour has no sRGB form.
fn blend(first: &NSColor, second: &NSColor, amount: CGFloat) -> Retained<NSColor> {
    let (Some(lhs), Some(rhs)) = (first.colorUsingColorSpace(&srgb()), second.colorUsingColorSpace(&srgb())) else {
        return first.retain();
    };
    let t = amount.max(0.0).min(1.0);
    NSColor::colorWithSRGBRed_green_blue_alpha(
        lhs.redComponent() + (rhs.redComponent() - lhs.redComponent()) * t,
        lhs.greenComponent() + (rhs.greenComponent() - lhs.greenComponent()) * t,
        lhs.blueComponent() + (rhs.blueComponent() - lhs.blueComponent()) * t,
        lhs.alphaComponent() + (rhs.alphaComponent() - lhs.alphaComponent()) * t,
    )
}

/// Schemes a Markdown document may legitimately hand to a browser as an
/// active link. Anything else is rendered as inert text.
const LINK_SCHEME_ALLOWLIST: [&str; 3] = ["http", "https", "mailto"];

impl HTMLExporter {
    /// Largest local asset inlined as a data URI.
    pub const MAX_EMBEDDED_ASSET_BYTES: i64 = 5 * 1024 * 1024;

    pub fn new(
        document: Arc<ParsedDocument>,
        theme: Theme,
        title: impl Into<String>,
        base_directory: Option<FileUrl>,
        image_provider: Option<Box<dyn FragmentImageProvider>>,
    ) -> HTMLExporter {
        HTMLExporter { document, theme, title: title.into(), base_directory, image_provider, for_print: false }
    }

    pub fn html(&self) -> String {
        let mut body = String::with_capacity(self.document.text.len() * 2);
        for child in &self.document.root.children {
            self.render_block(child, &mut body);
        }
        let stylesheet = self.stylesheet();
        let mut out = String::with_capacity(body.len() + stylesheet.len() + 400);
        out.push_str(
            "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
             <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>",
        );
        escape_into(&self.title, &mut out);
        out.push_str("</title>\n<style>\n");
        out.push_str(&stylesheet);
        out.push_str("\n</style>\n</head>\n<body>\n<article class=\"downright\">\n");
        out.push_str(&body);
        out.push_str("\n</article>\n</body>\n</html>");
        out
    }

    // MARK: - Blocks

    fn render_children(&self, block: &MDBlock, out: &mut String) {
        for child in &block.children {
            self.render_block(child, out);
        }
    }

    fn render_block(&self, block: &MDBlock, out: &mut String) {
        let document = &*self.document;
        match &block.content {
            BlockContent::Document => self.render_children(block, out),

            BlockContent::Heading { level } => {
                let mut text = String::new();
                self.render_inlines(&block.inlines, block.content_range, &mut text);
                let raw_text = document.substring(block.content_range);
                let slug = match document.headings.iter().find(|heading| heading.range == block.range) {
                    Some(heading) => heading.slug.clone(),
                    None => Slugs::make(&raw_text),
                };
                out.push_str(&format!("<h{level} id=\""));
                escape_into(&slug, out);
                out.push_str("\">");
                out.push_str(&text);
                out.push_str(&format!("</h{level}>\n"));
            }

            BlockContent::Paragraph => {
                out.push_str("<p>");
                self.render_inlines(&block.inlines, block.content_range, out);
                out.push_str("</p>\n");
            }

            BlockContent::BlockQuote => {
                out.push_str("<blockquote>\n");
                self.render_children(block, out);
                out.push_str("</blockquote>\n");
            }

            BlockContent::Callout { kind, title } => {
                let heading = match title {
                    Some(title) => title.clone(),
                    None => swift::capitalized(kind.raw_value()),
                };
                out.push_str("<div class=\"callout callout-");
                out.push_str(kind.raw_value());
                out.push_str("\">\n<div class=\"callout-title\">");
                escape_into(&heading, out);
                out.push_str("</div>\n");
                self.render_children(block, out);
                out.push_str("</div>\n");
            }

            BlockContent::List { ordered, start, .. } => {
                let tag = if *ordered { "ol" } else { "ul" };
                out.push('<');
                out.push_str(tag);
                if *ordered && *start != 1 {
                    out.push_str(&format!(" start=\"{start}\""));
                }
                out.push_str(">\n");
                self.render_children(block, out);
                out.push_str("</");
                out.push_str(tag);
                out.push_str(">\n");
            }

            BlockContent::ListItem { checkbox, .. } => {
                let mut inner = String::new();
                self.render_children(block, &mut inner);
                match checkbox {
                    None => {
                        out.push_str("<li>");
                        out.push_str(unwrap_tight_paragraph(&inner));
                        out.push_str("</li>\n");
                    }
                    Some(checkbox) => {
                        out.push_str("<li class=\"task\"><input type=\"checkbox\" disabled");
                        if checkbox.is_checked {
                            out.push_str(" checked");
                        }
                        out.push_str("> ");
                        out.push_str(unwrap_tight_paragraph(&inner));
                        out.push_str("</li>\n");
                    }
                }
            }

            BlockContent::CodeBlock { language, content_range, .. } => {
                let code = document.substring(*content_range);
                out.push_str("<div class=\"code\">");
                if let Some(language) = language {
                    out.push_str("<div class=\"code-lang\">");
                    escape_into(language, out);
                    out.push_str("</div>");
                }
                out.push_str("<pre><code");
                if let Some(language) = language {
                    out.push_str(" class=\"language-");
                    escape_into(language, out);
                    out.push('"');
                }
                out.push('>');
                escape_into(&code, out);
                out.push_str("</code></pre></div>\n");
            }

            BlockContent::Mermaid { source_range } => {
                let source = document.substring(*source_range);
                if let Some(uri) = self
                    .image_provider
                    .as_ref()
                    .and_then(|provider| provider.image_for_mermaid(&source, &self.theme))
                    .and_then(|image| data_uri(&image))
                {
                    out.push_str("<figure class=\"diagram\"><img src=\"");
                    out.push_str(&uri);
                    out.push_str("\" alt=\"Diagram\"></figure>\n");
                    return;
                }
                out.push_str("<div class=\"code\"><pre><code>");
                escape_into(&source, out);
                out.push_str("</code></pre></div>\n");
            }

            BlockContent::MathBlock { latex_range } => {
                let latex = document.substring(*latex_range);
                if let Some(uri) = self
                    .image_provider
                    .as_ref()
                    .and_then(|provider| {
                        provider.image_for_math(
                            &latex,
                            true,
                            self.theme.typography.body_size * 1.1,
                            &self.theme.palette.text.resolved(),
                        )
                    })
                    .and_then(|image| data_uri(&image))
                {
                    out.push_str("<figure class=\"math\"><img src=\"");
                    out.push_str(&uri);
                    out.push_str("\" alt=\"");
                    escape_into(&latex, out);
                    out.push_str("\"></figure>\n");
                    return;
                }
                out.push_str("<figure class=\"math\"><code>");
                escape_into(&latex, out);
                out.push_str("</code></figure>\n");
            }

            BlockContent::Table(data) => self.render_table(data, out),

            BlockContent::ThematicBreak => out.push_str("<hr>\n"),

            BlockContent::HtmlBlock => {
                // Exported documents are standalone files opened outside the
                // app; a raw block that shipped verbatim would be a stored XSS
                // hole. Render it as escaped source text.
                let html = document.substring(block.range);
                out.push_str("<div class=\"code\"><pre><code>");
                escape_into(&html, out);
                out.push_str("</code></pre></div>\n");
            }

            BlockContent::FrontMatter(matter) => {
                if matter.fields.is_empty() {
                    return;
                }
                out.push_str("<div class=\"frontmatter\">");
                for field in &matter.fields {
                    out.push_str("<div class=\"fm-row\"><span class=\"fm-key\">");
                    escape_into(&field.key, out);
                    out.push_str("</span><span class=\"fm-value\">");
                    escape_into(&field.value, out);
                    out.push_str("</span></div>");
                }
                out.push_str("</div>\n");
            }

            BlockContent::FootnoteDefinition { identifier } => {
                let mut inner = String::new();
                self.render_children(block, &mut inner);
                out.push_str("<div class=\"footnote\" id=\"fn-");
                escape_into(identifier, out);
                out.push_str("\"><sup>");
                escape_into(identifier, out);
                out.push_str("</sup>");
                out.push_str(&inner);
                out.push_str("</div>\n");
            }
        }
    }

    fn render_table(&self, table: &TableData, out: &mut String) {
        out.push_str("<table>\n");
        if let Some(header) = table.header_row() {
            out.push_str("<thead><tr>");
            for (index, cell) in header.cells.iter().enumerate() {
                out.push_str("<th");
                out.push_str(alignment_attribute(table, index));
                out.push('>');
                self.render_inlines(&cell.inlines, cell.content_range, out);
                out.push_str("</th>");
            }
            out.push_str("</tr></thead>\n");
        }
        out.push_str("<tbody>\n");
        for row in table.body_rows() {
            out.push_str("<tr>");
            for (index, cell) in row.cells.iter().enumerate() {
                out.push_str("<td");
                out.push_str(alignment_attribute(table, index));
                out.push('>');
                self.render_inlines(&cell.inlines, cell.content_range, out);
                out.push_str("</td>");
            }
            out.push_str("</tr>\n");
        }
        out.push_str("</tbody>\n</table>\n");
    }

    // MARK: - Inlines

    fn substring_into(&self, range: NSRange, out: &mut String) {
        escape_into(&self.document.substring(range), out);
    }

    fn render_inlines(&self, spans: &[InlineSpan], range: NSRange, out: &mut String) {
        if spans.is_empty() {
            self.substring_into(range, out);
            return;
        }
        // `spans.sorted(by: <location)`: Swift's sort is stable, as is this.
        let sorted: std::borrow::Cow<'_, [InlineSpan]> =
            if spans.is_sorted_by_key(|span| span.range.location) {
                std::borrow::Cow::Borrowed(spans)
            } else {
                let mut spans = spans.to_vec();
                spans.sort_by_key(|span| span.range.location);
                std::borrow::Cow::Owned(spans)
            };
        let mut cursor = range.location;
        for span in sorted.iter() {
            if span.range.location > cursor {
                self.substring_into(NSRange::new(cursor, span.range.location - cursor), out);
            }
            self.render_inline(span, out);
            cursor = cursor.max(span.range.upper_bound());
        }
        if cursor < range.upper_bound() {
            self.substring_into(NSRange::new(cursor, range.upper_bound() - cursor), out);
        }
    }

    fn render_inline(&self, span: &InlineSpan, out: &mut String) {
        let inner = |out: &mut String| self.render_inlines(&span.children, span.content_range, out);
        match &span.kind {
            InlineKind::Text => self.substring_into(span.range, out),
            InlineKind::Emphasis => {
                out.push_str("<em>");
                inner(out);
                out.push_str("</em>");
            }
            InlineKind::Strong => {
                out.push_str("<strong>");
                inner(out);
                out.push_str("</strong>");
            }
            InlineKind::Strikethrough => {
                out.push_str("<del>");
                inner(out);
                out.push_str("</del>");
            }
            InlineKind::InlineCode => {
                out.push_str("<code>");
                self.substring_into(span.content_range, out);
                out.push_str("</code>");
            }
            InlineKind::Link { destination, title } => {
                // A link whose scheme is missing from the allowlist must never
                // become an active `href`: no anchor, destination as tooltip.
                if !link_is_safe(destination) {
                    out.push_str("<span title=\"");
                    escape_into(destination, out);
                    out.push_str("\">");
                    inner(out);
                    out.push_str("</span>");
                    return;
                }
                out.push_str("<a href=\"");
                escape_into(&resolve_href(destination), out);
                out.push('"');
                if let Some(title) = title {
                    out.push_str(" title=\"");
                    escape_into(title, out);
                    out.push('"');
                }
                out.push('>');
                inner(out);
                out.push_str("</a>");
            }
            InlineKind::Autolink { destination } => {
                if !link_is_safe(destination) {
                    escape_into(destination, out);
                    return;
                }
                out.push_str("<a href=\"");
                escape_into(destination, out);
                out.push_str("\">");
                escape_into(destination, out);
                out.push_str("</a>");
            }
            InlineKind::Wikilink { target, label } => {
                let text = label.as_deref().unwrap_or(target);
                if !link_is_safe(target) {
                    out.push_str("<span class=\"wikilink\" title=\"");
                    escape_into(target, out);
                    out.push_str("\">");
                    escape_into(text, out);
                    out.push_str("</span>");
                    return;
                }
                out.push_str("<a class=\"wikilink\" href=\"");
                escape_into(target, out);
                out.push_str(".html\">");
                escape_into(text, out);
                out.push_str("</a>");
            }
            InlineKind::Image { source, alt } => self.render_image(source, alt, out),
            InlineKind::InlineMath { latex_range } => {
                let latex = self.document.substring(*latex_range);
                if let Some(uri) = self
                    .image_provider
                    .as_ref()
                    .and_then(|provider| {
                        provider.image_for_math(
                            &latex,
                            false,
                            self.theme.typography.body_size,
                            &self.theme.palette.text.resolved(),
                        )
                    })
                    .and_then(|image| data_uri(&image))
                {
                    out.push_str("<img class=\"inline-math\" src=\"");
                    out.push_str(&uri);
                    out.push_str("\" alt=\"");
                    escape_into(&latex, out);
                    out.push_str("\">");
                    return;
                }
                out.push_str("<code class=\"inline-math\">");
                escape_into(&latex, out);
                out.push_str("</code>");
            }
            InlineKind::PathToken(token) => {
                out.push_str("<code class=\"path\">");
                escape_into(&token.raw_path, out);
                out.push_str("</code>");
            }
            InlineKind::FootnoteReference { identifier } => {
                out.push_str("<sup><a href=\"#fn-");
                escape_into(identifier, out);
                out.push_str("\">");
                escape_into(identifier, out);
                out.push_str("</a></sup>");
            }
            InlineKind::SoftBreak => out.push('\n'),
            InlineKind::LineBreak => out.push_str("<br>\n"),
            // Inline HTML is source text in an export.
            InlineKind::InlineHTML => self.substring_into(span.range, out),
        }
    }

    fn render_image(&self, source: &str, alt: &str, out: &mut String) {
        let caption = if alt.is_empty() {
            String::new()
        } else {
            let mut caption = String::from("<figcaption>");
            escape_into(alt, &mut caption);
            caption.push_str("</figcaption>");
            caption
        };
        // A self-contained export never retains a network, data, custom-scheme
        // or file URL; only confined relative assets become live sources.
        if has_url_scheme(source) || swift::has_prefix(source, "//") {
            return missing_image(source, &caption, out);
        }
        // An unsaved document has no asset root.
        let Some(base) = &self.base_directory else {
            return missing_image(source, &caption, out);
        };
        let Some(url) = confined_url(source, base) else {
            return missing_image(source, &caption, out);
        };
        let size = file_size(Path::new(&url.path())).unwrap_or(i64::MAX);
        let data = if size > 0 && size <= Self::MAX_EMBEDDED_ASSET_BYTES { std::fs::read(url.path()).ok() } else { None };
        let Some(data) = data else {
            return missing_image(source, &caption, out);
        };
        let mime = Self::mime_type(&url.path_extension());
        out.push_str("<figure><img src=\"data:");
        out.push_str(mime);
        out.push_str(";base64,");
        base64_into(&data, out);
        out.push_str("\" alt=\"");
        escape_into(alt, out);
        out.push_str("\">");
        out.push_str(&caption);
        out.push_str("</figure>");
    }

    pub fn mime_type(extension: &str) -> &'static str {
        match swift::lowercased(extension).as_str() {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "svg" => "image/svg+xml",
            "webp" => "image/webp",
            "heic" => "image/heic",
            "pdf" => "application/pdf",
            _ => "application/octet-stream",
        }
    }

    // MARK: - Stylesheet

    fn stylesheet(&self) -> String {
        let palette = &self.theme.palette;
        let typography = &self.theme.typography;
        let body_family = if typography.preset == BodyPreset::Reading {
            "'New York', 'Iowan Old Style', Georgia, serif"
        } else {
            "-apple-system, 'SF Pro Text', system-ui, sans-serif"
        };
        let scale = typography.scale_ratio;
        let size = |steps: i32| format_double("%.3frem", scale.powf(steps as f64));
        let size_exponent = |exponent: f64| format_double("%.3frem", scale.powf(exponent));

        let heading_color = hex(&palette.heading);
        let heading_secondary = palette.text_secondary.resolved();
        let heading4_color = hex_color(&blend(&palette.heading.resolved(), &heading_secondary, 0.25));
        let heading5_color = hex_color(&blend(&palette.heading.resolved(), &heading_secondary, 0.5));
        let heading6_color = hex_color(&blend(&palette.heading.resolved(), &heading_secondary, 0.75));

        let page_rule = if self.for_print {
            "@page { margin: 20mm 18mm; }\nbody { background: #fff; }\n.downright { max-width: none; }\n\
             pre, blockquote, table, figure { break-inside: avoid; }\nh1, h2, h3 { break-after: avoid; }\n\
             a { color: inherit; text-decoration: none; }\n\
             a[href^=\"http\"]::after { content: \" (\" attr(href) \")\"; font-size: 0.8em; color: #666; }"
        } else {
            ""
        };

        let mut out = String::with_capacity(6000);
        out.push_str(":root { color-scheme: ");
        out.push_str(if self.theme.appearance == ThemeAppearance::Dark { "dark" } else { "light" });
        out.push_str("; }\n* { box-sizing: border-box; }\nbody {\n  margin: 0;\n  background: ");
        out.push_str(&hex(&palette.background));
        out.push_str(";\n  color: ");
        out.push_str(&hex(&palette.text));
        out.push_str(";\n  font-family: ");
        out.push_str(body_family);
        out.push_str(";\n  font-size: ");
        out.push_str(&double_description(typography.body_size));
        out.push_str("px;\n  line-height: ");
        out.push_str(&double_description(typography.line_height_multiple));
        out.push_str(";\n  -webkit-font-smoothing: antialiased;\n}\n.downright {\n  /* Measure capped in ch so it tracks the font, not the window (§11.1). */\n  max-width: ");
        out.push_str(&swift_int(typography.measure_characters).to_string());
        out.push_str("ch;\n  margin: 4rem auto;\n  padding: 0 1.5rem;\n  hanging-punctuation: first last;\n}\nh1, h2, h3, h4, h5, h6 {\n  line-height: 1.2;\n  margin: 1.8em 0 0.6em;\n}\nh1 { color: ");
        out.push_str(&heading_color);
        out.push_str("; font-size: ");
        out.push_str(&size(3));
        out.push_str("; margin-top: 0; font-weight: 700; letter-spacing: -0.022em; }\nh2 { color: ");
        out.push_str(&heading_color);
        out.push_str("; font-size: ");
        out.push_str(&size(2));
        out.push_str("; font-weight: 700; letter-spacing: -0.014em; }\nh3 { color: ");
        out.push_str(&heading_color);
        out.push_str("; font-size: ");
        out.push_str(&size_exponent(1.25));
        out.push_str("; font-weight: 700; letter-spacing: -0.014em; }\nh4 { color: ");
        out.push_str(&heading4_color);
        out.push_str("; font-size: ");
        out.push_str(&size_exponent(0.5));
        out.push_str("; font-weight: 600; letter-spacing: normal; }\nh5 {\n  color: ");
        out.push_str(&heading5_color);
        out.push_str("; font-size: ");
        out.push_str(&size_exponent(-0.5));
        out.push_str(";\n  font-weight: 600; letter-spacing: 0.04em;\n}\nh6 {\n  color: ");
        out.push_str(&heading6_color);
        out.push_str("; font-size: ");
        out.push_str(&size_exponent(-0.75));
        out.push_str(";\n  font-weight: 500; font-style: italic; letter-spacing: 0.06em;\n}\np { margin: 0 0 1.1em; }\na { color: ");
        out.push_str(&hex(&palette.link));
        out.push_str("; text-decoration-thickness: 1px; text-underline-offset: 2px; }\nstrong { font-weight: 640; }\n/* Left rule, never a filled box (§11.3). */\nblockquote {\n  margin: 1.4em 0; padding: 0.1em 0 0.1em 1.1em;\n  border-left: 2px solid ");
        out.push_str(&hex(&palette.quote_rule));
        out.push_str(";\n  color: ");
        out.push_str(&hex(&palette.text_secondary));
        out.push_str(";\n}\n.callout {\n  margin: 1.4em 0; padding: 0.2em 0 0.2em 1.1em;\n  border-left: 3px solid ");
        out.push_str(&hex(&palette.callout_note));
        out.push_str(";\n}\n.callout-title { font-weight: 640; margin-bottom: 0.3em; letter-spacing: 0.01em; }\n.callout-warning { border-left-color: ");
        out.push_str(&hex(&palette.callout_warning));
        out.push_str("; }\n.callout-warning .callout-title { color: ");
        out.push_str(&hex(&palette.callout_warning));
        out.push_str("; }\n.callout-caution, .callout-danger, .callout-bug { border-left-color: ");
        out.push_str(&hex(&palette.callout_danger));
        out.push_str("; }\n.callout-caution .callout-title, .callout-danger .callout-title { color: ");
        out.push_str(&hex(&palette.callout_danger));
        out.push_str("; }\n.callout-tip, .callout-success { border-left-color: ");
        out.push_str(&hex(&palette.callout_success));
        out.push_str("; }\n.callout-note .callout-title, .callout-info .callout-title { color: ");
        out.push_str(&hex(&palette.callout_note));
        out.push_str("; }\n.callout-important { border-left-color: ");
        out.push_str(&hex(palette.callout_important.as_ref().unwrap_or(&palette.callout_danger)));
        out.push_str("; }\n.callout-important .callout-title { color: ");
        out.push_str(&hex(palette.callout_important.as_ref().unwrap_or(&palette.callout_danger)));
        out.push_str("; }\n/* Subtle tint plus a left rule, never a heavy bordered card (§11.3). */\n.code {\n  position: relative; margin: 1.4em 0;\n  background: ");
        out.push_str(&hex(&palette.code_background));
        out.push_str(";\n  border-left: 2px solid ");
        out.push_str(&hex(&palette.code_rule));
        out.push_str(";\n  border-radius: 0 4px 4px 0;\n}\n.code-lang {\n  position: absolute; top: 0.5em; right: 0.8em;\n  font-size: 0.7rem; letter-spacing: 0.04em; text-transform: uppercase;\n  color: ");
        out.push_str(&hex(&palette.text_faint));
        out.push_str(";\n}\npre { margin: 0; padding: 0.9em 1.1em; overflow-x: auto; }\ncode {\n  font-family: 'SF Mono', ui-monospace, Menlo, monospace;\n  font-size: ");
        out.push_str(&double_description(typography.mono_size_adjust));
        out.push_str("em;\n}\np code, li code, td code {\n  background: ");
        out.push_str(&hex(&palette.code_background));
        out.push_str(";\n  padding: 0.12em 0.35em; border-radius: 3px;\n}\ncode.path { color: ");
        out.push_str(&hex(&palette.accent));
        out.push_str("; }\n/* Horizontal rules only, zebra on hover, no gridlines (§11.3). */\ntable { width: 100%; border-collapse: collapse; margin: 1.5em 0; font-size: 0.95em; }\nth, td { padding: 0.5em 0.7em; text-align: left; }\nth {\n  border-bottom: 1.5px solid ");
        out.push_str(&hex(&palette.rule));
        out.push_str(";\n  font-weight: 620; color: ");
        out.push_str(&hex(&palette.text_secondary));
        out.push_str(";\n  font-size: 0.82em; letter-spacing: 0.03em; text-transform: uppercase;\n}\ntd { border-bottom: 1px solid ");
        out.push_str(&hex(&palette.rule));
        out.push_str("33; }\ntbody tr:hover { background: ");
        out.push_str(&hex(&palette.code_background));
        out.push_str("; }\n/* Hairline with generous space, not a thick divider (§11.3). */\nhr { border: none; border-top: 1px solid ");
        out.push_str(&hex(&palette.rule));
        out.push_str("; margin: 3em 0; }\nul, ol { padding-left: 1.3em; margin: 0 0 1.1em; }\nli { margin: 0.25em 0; }\nli.task { list-style: none; margin-left: -1.1em; }\nli.task input { margin-right: 0.45em; }\nfigure { margin: 1.6em 0; text-align: center; }\nfigure img { max-width: 100%; border-radius: 6px; box-shadow: 0 1px 6px rgba(0,0,0,0.10); }\nfigcaption { margin-top: 0.6em; font-size: 0.85em; color: ");
        out.push_str(&hex(&palette.text_secondary));
        out.push_str("; }\nfigure.missing span { color: ");
        out.push_str(&hex(&palette.path_missing));
        out.push_str("; font-family: 'SF Mono', monospace; font-size: 0.85em; }\n.inline-math { vertical-align: -0.18em; height: 1.05em; box-shadow: none; border-radius: 0; }\nfigure.math img { box-shadow: none; }\n.frontmatter {\n  margin: 0 0 2.4em; padding: 0.9em 1.1em;\n  background: ");
        out.push_str(&hex(&palette.code_background));
        out.push_str("; border-radius: 6px;\n  font-size: 0.88em;\n}\n.fm-row { display: flex; gap: 0.8em; padding: 0.15em 0; }\n.fm-key { color: ");
        out.push_str(&hex(&palette.text_secondary));
        out.push_str("; min-width: 7em; }\n.footnote { font-size: 0.88em; color: ");
        out.push_str(&hex(&palette.text_secondary));
        out.push_str("; }\n.footnote sup { margin-right: 0.4em; color: ");
        out.push_str(&hex(&palette.accent));
        out.push_str("; }\n");
        out.push_str(page_rule);
        out
    }
}

/// `alignmentAttribute(_:_:)`.
fn alignment_attribute(table: &TableData, column: usize) -> &'static str {
    match table.alignments.get(column) {
        None | Some(TableAlignment::None) => "",
        Some(TableAlignment::Left) => " style=\"text-align:left\"",
        Some(TableAlignment::Center) => " style=\"text-align:center\"",
        Some(TableAlignment::Right) => " style=\"text-align:right\"",
    }
}

/// A tight list item wraps its text in `<p>`, which browsers render with
/// list-item margins the source never asked for. Character-wise, as Swift's
/// `hasPrefix`, `hasSuffix`, `dropFirst`, `dropLast` and `contains` are.
fn unwrap_tight_paragraph(html: &str) -> &str {
    let trimmed = swift::trim_whitespaces_and_newlines(html);
    if !(swift::has_prefix(trimmed, "<p>") && swift::has_suffix(trimmed, "</p>")) {
        return html;
    }
    // Both markers matched Character by Character, so they are exactly
    // three and four bytes.
    let inner = &trimmed[3..trimmed.len() - 4];
    if swift::contains(inner, "<p>") {
        return html;
    }
    inner
}

/// `resolveHref(_:)`: a relative link to another Markdown file points at
/// that file's exported sibling.
fn resolve_href(destination: &str) -> String {
    if has_url_scheme(destination) || swift::has_prefix(destination, "//") {
        return destination.to_owned();
    }
    let suffix_start = swift::first_index_where(destination, |character| {
        swift::char_is(character, '?') || swift::char_is(character, '#')
    })
    .unwrap_or(destination.len());
    let path = &destination[..suffix_start];
    if !swift::has_suffix(path, ".md") {
        return destination.to_owned();
    }
    format!("{}.html{}", &path[..path.len() - 3], &destination[suffix_start..])
}

/// `linkIsSafe(_:)`: no scheme at all (a relative path or `#anchor`), or an
/// allow-listed one.
fn link_is_safe(destination: &str) -> bool {
    let lower = swift::lowercased(destination);
    let Some(colon) = swift::first_index_of(&lower, ':') else { return true };
    let before = &lower[..colon];
    // A colon inside a *path* is preceded by a `/`, so such destinations
    // still parse as path content.
    if before.is_empty() || before.chars().any(|c| matches!(c, '/' | '?' | '#' | '\\')) {
        return true;
    }
    LINK_SCHEME_ALLOWLIST.contains(&before)
}

/// `hasURLScheme(_:)`: a non-empty prefix before the first `:` made only of
/// alphanumerics and `+-.`.
fn has_url_scheme(source: &str) -> bool {
    let Some(colon) = swift::first_index_of(source, ':') else { return false };
    let prefix = &source[..colon];
    if prefix.is_empty() {
        return false;
    }
    prefix.chars().all(|c| CharSet::Alphanumerics.contains(c) || matches!(c, '+' | '-' | '.'))
}

/// `confinedURL(for:relativeTo:)`: resolves a relative image source against
/// the base directory only if the result, symbolic links followed, stays
/// inside it.
fn confined_url(source: &str, base: &FileUrl) -> Option<FileUrl> {
    if swift::has_prefix(source, "/") {
        return None;
    }
    let candidate = base
        .appending_path_component(source)
        .standardized_file_url()
        .resolving_symlinks_in_path()
        .standardized_file_url();
    let base_path = base.resolving_symlinks_in_path().standardized_file_url().path();
    let prefix = if swift::has_suffix(&base_path, "/") { base_path } else { base_path + "/" };
    if swift::has_prefix(&candidate.path(), &prefix) { Some(candidate) } else { None }
}

fn missing_image(source: &str, caption: &str, out: &mut String) {
    out.push_str("<figure class=\"missing\"><span>");
    escape_into(source, out);
    out.push_str("</span>");
    out.push_str(caption);
    out.push_str("</figure>");
}

/// `dataURI(for:)`: TIFF → `NSBitmapImageRep` → PNG, base64.
fn data_uri(image: &NSImage) -> Option<String> {
    objc2::rc::autoreleasepool(|_| {
        let tiff = image.TIFFRepresentation()?;
        let bitmap = NSBitmapImageRep::imageRepWithData(&tiff)?;
        let png = unsafe { bitmap.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()) }?;
        let mut uri = String::from("data:image/png;base64,");
        base64_into(&png.to_vec(), &mut uri);
        Some(uri)
    })
}

/// `Data.base64EncodedString()`: standard alphabet, padded, no line breaks.
pub fn base64_into(data: &[u8], out: &mut String) {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    out.reserve(data.len().div_ceil(3) * 4);
    let mut chunks = data.chunks_exact(3);
    for chunk in &mut chunks {
        let n = (chunk[0] as u32) << 16 | (chunk[1] as u32) << 8 | chunk[2] as u32;
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(ALPHABET[(n >> 6) as usize & 63] as char);
        out.push(ALPHABET[n as usize & 63] as char);
    }
    match chunks.remainder() {
        [a] => {
            let n = (*a as u32) << 16;
            out.push(ALPHABET[(n >> 18) as usize & 63] as char);
            out.push(ALPHABET[(n >> 12) as usize & 63] as char);
            out.push_str("==");
        }
        [a, b] => {
            let n = (*a as u32) << 16 | (*b as u32) << 8;
            out.push(ALPHABET[(n >> 18) as usize & 63] as char);
            out.push(ALPHABET[(n >> 12) as usize & 63] as char);
            out.push(ALPHABET[(n >> 6) as usize & 63] as char);
            out.push('=');
        }
        _ => {}
    }
}

/// `escape(_:)`: `& < > "` become entities, Character by Character. A
/// Character is one of those only when it is that single scalar, so `<`
/// followed by a combining mark (one Character) is copied as it is.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    escape_into(text, &mut out);
    out
}

fn escape_into(text: &str, out: &mut String) {
    let bytes = text.as_bytes();
    let special = |b: u8| matches!(b, b'&' | b'<' | b'>' | b'"');
    if text.is_ascii() {
        // Every ASCII Character is one scalar except CR LF, which is not
        // special.
        let mut start = 0;
        for (index, &byte) in bytes.iter().enumerate() {
            if special(byte) {
                out.push_str(&text[start..index]);
                out.push_str(entity(byte));
                start = index + 1;
            }
        }
        out.push_str(&text[start..]);
        return;
    }
    for character in swift::graphemes(text) {
        if character.len() == 1 && special(character.as_bytes()[0]) {
            out.push_str(entity(character.as_bytes()[0]));
        } else {
            out.push_str(character);
        }
    }
}

fn entity(byte: u8) -> &'static str {
    match byte {
        b'&' => "&amp;",
        b'<' => "&lt;",
        b'>' => "&gt;",
        _ => "&quot;",
    }
}

/// GitHub-compatible heading slugs, shared by export and by "copy link to
/// section" (§7.1).
pub struct Slugs;

impl Slugs {
    pub fn make(title: &str) -> String {
        let mut out = String::new();
        for scalar in swift::lowercased(title).chars() {
            if CharSet::Alphanumerics.contains(scalar) {
                out.push(scalar);
            } else if scalar == ' ' || scalar == '-' || scalar == '_' {
                out.push('-');
            }
        }
        while swift::contains(&out, "--") {
            out = swift::replacing_occurrences(&out, "--", "-");
        }
        swift::trimming(&out, CharSet::Chars("-")).to_owned()
    }
}
