//! The Rust counterparts of `MarkdownScene` and `LayoutDump` in
//! `oracle/Sources/downright-oracle/RenderCapture.swift`: Upleft's real
//! `MarkdownContainerView`, set up the way Downright's document window sets
//! up a document, and the geometry of every laid-out fragment.

use std::ptr::NonNull;
use std::rc::Rc;

use block2::StackBlock;
use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2::{AnyThread, MainThreadMarker};
use objc2_app_kit::{
    NSBitmapImageRep, NSTextElementProvider, NSTextLayoutFragment, NSTextLayoutFragmentEnumerationOptions,
    NSTextSelectionDataSource, NSTextStorage, NSView, NSWindow,
};
use objc2_core_foundation::CGRect;
use objc2_foundation::{NSRect, NSString};
use serde_json::Value;
use upleft_core::DirtySet;
use upleft_core::parser::MarkdownParser;
use upleft_render::render_contracts::RenderMode;
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_render::theme::theme_store::ThemeStore;
use upleft_render::view::markdown_container_view::MarkdownContainerView;
use upleft_render::view::markdown_text_view::MarkdownTextView;
use upleft_render::view::markdown_text_view_delegate::ScrollPosition;

use super::json::{Object, double};
use super::density::DensityHost;
use crate::capture::{CaptureRequest, CaptureScene, appearance};

/// Downright's real `MarkdownContainerView` (`MarkdownScene`).
pub struct MarkdownScene {
    mode: RenderMode,
    theme_name: String,
    container: Option<Retained<MarkdownContainerView>>,
    /// `--density leading|trailing` (see `DensityHost`).
    density: Option<String>,
    density_host: Option<Rc<DensityHost>>,
}

impl MarkdownScene {
    pub fn new(mode: &str, theme_name: &str) -> MarkdownScene {
        MarkdownScene {
            mode: RenderMode::from_raw_value(mode).unwrap_or(RenderMode::Live),
            theme_name: theme_name.to_owned(),
            container: None,
            density: None,
            density_host: None,
        }
    }

    pub fn with_density(mut self, density: Option<String>) -> MarkdownScene {
        self.density = density;
        self
    }

    pub fn density_host(&self) -> Option<&Rc<DensityHost>> {
        self.density_host.as_ref()
    }

    fn container(&self) -> &MarkdownContainerView {
        self.container.as_deref().expect("the scene is built")
    }
}

impl CaptureScene for MarkdownScene {
    fn build(&mut self, window: &NSWindow, request: &CaptureRequest, mtm: MainThreadMarker) -> Result<Retained<NSView>, String> {
        let text = super::markup::read_text(&request.input).map_err(|error| format!("{error:?}"))?;
        let appearance = appearance(request.dark);
        let themes = ThemeStore::shared().themes();
        let Some(theme) = themes.iter().find(|theme| theme.name == self.theme_name).cloned() else {
            let known: Vec<String> = themes.iter().map(|theme| theme.name.clone()).collect();
            return Err(format!("unknown theme {}; known: {}", self.theme_name, known.join(", ")));
        };
        let style_sheet = Rc::new(StyleSheet::new(theme, &appearance, Some(true)));
        let storage = NSTextStorage::from_nsstring_storage(&NSString::from_str(&text));
        let container = MarkdownContainerView::new(&storage, style_sheet.clone(), mtm);
        if let Some(density) = &self.density {
            self.density_host = Some(DensityHost::new(&container, density, style_sheet, &text, mtm));
        }
        container.setFrame(CGRect::new(
            objc2_foundation::NSPoint::new(0.0, 0.0),
            objc2_foundation::NSSize::new(request.width, request.height),
        ));
        window.setContentView(Some(&container));
        // As in the app: the container is laid out in its window before the
        // first document update resizes the text view to its content.
        window.layoutIfNeeded();
        container.layoutSubtreeIfNeeded();
        let text_view = container.text_view();
        text_view.set_mode(self.mode);
        let document = MarkdownParser::parse(&text);
        text_view.update(document.clone(), &DirtySet::wholesale(), true);
        if let Some(host) = &self.density_host {
            host.refresh_density_bands(document);
        }
        self.container = Some(container.clone());
        Ok(Retained::into_super(container))
    }

    /// The first-frame sequence of Downright's `DocumentWindowController`.
    fn after_show(&mut self, window: &NSWindow) {
        window.layoutIfNeeded();
        let container = self.container();
        container.layoutSubtreeIfNeeded();
        let text_view = container.text_view();
        text_view.resize_to_fit_content();
        text_view.scroll_to_offset(0, ScrollPosition::Top, false);
        text_view.prepare_for_display();
        text_view.displayIfNeeded();
        if let Some(host) = &self.density_host {
            host.update_gutter();
        }
    }

    fn before_settle_check(&mut self) {
        let container = self.container();
        container.layoutSubtreeIfNeeded();
        if let Some(layout) = container.text_view().textLayoutManager() {
            layout.ensureLayoutForRange(&layout.documentRange());
        }
    }

    fn write_extras(&mut self, bitmap: &NSBitmapImageRep, request: &CaptureRequest) -> Result<(), String> {
        let Some(path) = &request.output_layout else { return Ok(()) };
        let container = self.container();
        let dump = layout_dump(container.text_view(), container, bitmap);
        super::json::write(&dump, path).map_err(|error| error.to_string())
    }
}

fn rect_json(rect: NSRect) -> Value {
    Value::Array(vec![double(rect.origin.x), double(rect.origin.y), double(rect.size.width), double(rect.size.height)])
}

/// `LayoutDump.textView(_:container:bitmap:)`.
pub fn layout_dump(text_view: &MarkdownTextView, container: &MarkdownContainerView, bitmap: &NSBitmapImageRep) -> Value {
    let fragments: std::cell::RefCell<Vec<Value>> = std::cell::RefCell::new(Vec::new());
    if let Some(layout) = text_view.textLayoutManager()
        && let Some(content) = layout.textContentManager()
    {
        let document_start = content.documentRange().location();
        let block = StackBlock::new(|fragment: NonNull<NSTextLayoutFragment>| -> Bool {
            // SAFETY: TextKit hands a live fragment for the call.
            let fragment = unsafe { fragment.as_ref() };
            let range = fragment.rangeInElement();
            let start = content.offsetFromLocation_toLocation(&document_start, &range.location());
            let end = content.offsetFromLocation_toLocation(&document_start, &range.endLocation());
            let lines: Vec<Value> = fragment
                .textLineFragments()
                .iter()
                .map(|line| {
                    let character_range = line.characterRange();
                    let origin = line.glyphOrigin();
                    Object::new()
                        .with(
                            "characterRange",
                            Value::Array(vec![character_range.location.into(), character_range.length.into()]),
                        )
                        .with("typographicBounds", rect_json(line.typographicBounds()))
                        .with("glyphOrigin", Value::Array(vec![double(origin.x), double(origin.y)]))
                        .build()
                })
                .collect();
            fragments.borrow_mut().push(
                Object::new()
                    .with("class", fragment.class().name().to_string_lossy().into_owned())
                    .with("range", Value::Array(vec![start.into(), (end - start).into()]))
                    .with("frame", rect_json(fragment.layoutFragmentFrame()))
                    .with("renderingSurfaceBounds", rect_json(fragment.renderingSurfaceBounds()))
                    .with("lines", Value::Array(lines))
                    .build(),
            );
            Bool::YES
        });
        layout.enumerateTextLayoutFragmentsFromLocation_options_usingBlock(
            Some(&document_start),
            NSTextLayoutFragmentEnumerationOptions::EnsuresLayout,
            &block,
        );
    }
    let container_size = unsafe { text_view.textContainer() }.map(|container| container.size());
    Object::new()
        .with(
            "bitmap",
            Object::new()
                .with("pixelsWide", bitmap.pixelsWide())
                .with("pixelsHigh", bitmap.pixelsHigh())
                .with("bitsPerPixel", bitmap.bitsPerPixel())
                .with(
                    "colorSpace",
                    bitmap.colorSpace().localizedName().map(|name| name.to_string()).unwrap_or_default(),
                ),
        )
        .with("containerFrame", rect_json(container.frame()))
        .with("scrollViewFrame", rect_json(container.scroll_view().frame()))
        .with("textViewFrame", rect_json(text_view.frame()))
        .with(
            "textContainerSize",
            Value::Array(vec![
                double(container_size.map_or(0.0, |size| size.width)),
                double(container_size.map_or(0.0, |size| size.height)),
            ]),
        )
        .with("fragments", Value::Array(fragments.into_inner()))
        .build()
}

#[allow(dead_code)]
fn _traits(_: &dyn NSTextElementProvider, _: &dyn NSTextSelectionDataSource) {}

/// `NSTextStorage(string:)`.
trait TextStorageFromString {
    fn from_nsstring_storage(string: &NSString) -> Retained<NSTextStorage>;
}

impl TextStorageFromString for NSTextStorage {
    fn from_nsstring_storage(string: &NSString) -> Retained<NSTextStorage> {
        // SAFETY: `initWithString:` is NSTextStorage's (inherited) initialiser.
        unsafe { objc2::msg_send![NSTextStorage::alloc(), initWithString: string] }
    }
}
