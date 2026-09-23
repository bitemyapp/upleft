//! Port of `Theme.swift`: `DiagramTheme` and its derived colours. The named
//! built-in themes and the Shiki import are not on Downright's path; only
//! `.default` (zinc light) is kept.

use objc2::rc::Retained;
use objc2_app_kit::{NSColor, NSFont};
use objc2_core_foundation::CGFloat;

use crate::cross_platform::{color_from_hex, color_mix, mixed};
use crate::mermaid::src_types::SDict;
use crate::types::EdgeStyle;

#[derive(Debug, Clone)]
pub struct DiagramTheme {
    pub background: Retained<NSColor>,
    pub foreground: Retained<NSColor>,
    pub line: Option<Retained<NSColor>>,
    pub accent: Option<Retained<NSColor>>,
    pub muted: Option<Retained<NSColor>>,
    pub surface: Option<Retained<NSColor>>,
    pub border: Option<Retained<NSColor>>,
    pub font: Retained<NSFont>,
    pub line_width: CGFloat,
    pub corner_radius: CGFloat,
    /// When `true`, the diagram background is not filled.
    pub transparent: bool,
}

impl DiagramTheme {
    /// `DiagramTheme(background:foreground:)` with the other defaults
    /// (system font at 14 pt, line width 1.5, corner radius 8, opaque).
    pub fn new(background: Retained<NSColor>, foreground: Retained<NSColor>) -> DiagramTheme {
        DiagramTheme {
            background,
            foreground,
            line: None,
            accent: None,
            muted: None,
            surface: None,
            border: None,
            font: NSFont::systemFontOfSize(14.0),
            line_width: 1.5,
            corner_radius: 8.0,
            transparent: false,
        }
    }

    /// `DiagramTheme.default` (`zincLight`).
    pub fn zinc_light() -> DiagramTheme {
        DiagramTheme::new(color_from_hex("#FFFFFF"), color_from_hex("#27272A"))
    }

    pub fn effective_line(&self) -> Retained<NSColor> {
        self.line.clone().unwrap_or_else(|| mixed(&self.background, &self.foreground, color_mix::LINE))
    }
    pub fn effective_accent(&self) -> Retained<NSColor> {
        self.accent.clone().unwrap_or_else(|| self.foreground.clone())
    }
    pub fn effective_muted(&self) -> Retained<NSColor> {
        self.muted.clone().unwrap_or_else(|| mixed(&self.background, &self.foreground, color_mix::TEXT_MUTED))
    }
    pub fn effective_surface(&self) -> Retained<NSColor> {
        self.surface.clone().unwrap_or_else(|| mixed(&self.background, &self.foreground, color_mix::NODE_FILL))
    }
    pub fn effective_border(&self) -> Retained<NSColor> {
        self.border.clone().unwrap_or_else(|| mixed(&self.background, &self.foreground, color_mix::NODE_STROKE))
    }
    pub fn effective_text_secondary(&self) -> Retained<NSColor> {
        mixed(&self.background, &self.foreground, color_mix::TEXT_SEC)
    }
    pub fn effective_text_faint(&self) -> Retained<NSColor> {
        mixed(&self.background, &self.foreground, color_mix::TEXT_FAINT)
    }
    pub fn effective_arrow(&self) -> Retained<NSColor> {
        self.accent.clone().unwrap_or_else(|| mixed(&self.background, &self.foreground, color_mix::ARROW))
    }
    pub fn effective_inner_stroke(&self) -> Retained<NSColor> {
        mixed(&self.background, &self.foreground, color_mix::INNER_STROKE)
    }
    pub fn subgraph_background_color(&self) -> Retained<NSColor> {
        self.background.clone()
    }
    pub fn subgraph_header_color(&self) -> Retained<NSColor> {
        mixed(&self.background, &self.foreground, color_mix::GROUP_HEADER)
    }
    pub fn key_badge_color(&self) -> Retained<NSColor> {
        mixed(&self.background, &self.foreground, color_mix::KEY_BADGE)
    }

    pub fn edge_color(&self, style: &EdgeStyle) -> Retained<NSColor> {
        if let Some(hex) = &style.color {
            return color_from_hex(hex);
        }
        self.effective_line()
    }

    pub fn node_fill_color(&self, inline_styles: &SDict<String>) -> Retained<NSColor> {
        if let Some(fill) = inline_styles.get("fill") {
            return color_from_hex(fill);
        }
        self.effective_surface()
    }

    pub fn node_stroke_color(&self, inline_styles: &SDict<String>) -> Retained<NSColor> {
        if let Some(stroke) = inline_styles.get("stroke") {
            return color_from_hex(stroke);
        }
        self.effective_border()
    }

    pub fn node_text_color(&self, inline_styles: &SDict<String>) -> Retained<NSColor> {
        if let Some(color) = inline_styles.get("color") {
            return color_from_hex(color);
        }
        self.foreground.clone()
    }
}
