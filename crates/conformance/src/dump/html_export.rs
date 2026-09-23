//! Rust side of the `html-export` suite; mirrors
//! `oracle/app/Sources/downright-app-oracle/HTMLExportDump.swift` (see there
//! for the flags and the output).

use std::cell::RefCell;
use std::path::Path;

use block2::RcBlock;
use serde_json::Value;
use upleft_app::export::html_exporter::{HTMLExporter, NativeFragmentImageProvider};
use upleft_core::document_io::DocumentIO;
use upleft_core::parser::MarkdownParser;
use upleft_foundation::url::FileUrl;
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_render::theme::theme_store::ThemeStore;

use super::json::{self, Object};
use super::style_sheet::appearance_named;
use super::{Failure, Request};

pub struct Flags {
    pub theme: String,
    pub dark: bool,
    pub print: bool,
}

pub fn flags(arguments: &[String]) -> Result<Flags, Failure> {
    let mut flags = Flags { theme: "Paper Light".into(), dark: false, print: false };
    let mut iterator = arguments.iter();
    while let Some(flag) = iterator.next() {
        match flag.as_str() {
            "--theme" => {
                flags.theme =
                    iterator.next().ok_or_else(|| Failure::Error("--theme needs a value".into()))?.clone();
            }
            "--dark" => flags.dark = true,
            "--print" => flags.print = true,
            other => return Err(Failure::Error(format!("unknown flag {other}"))),
        }
    }
    Ok(flags)
}

/// The exporter the window controller builds for `input`, run under the
/// flags' appearance.
pub fn html(input: &Path, flags: &Flags) -> Result<String, Failure> {
    let store = ThemeStore::shared();
    let Some(theme) = store.themes().into_iter().find(|theme| theme.name == flags.theme) else {
        return Err(Failure::Error(format!("unknown theme {}", flags.theme)));
    };
    let appearance = appearance_named(flags.dark);
    let style_sheet = StyleSheet::new(theme, &appearance, Some(true));
    let url = FileUrl::from_path(&input.to_string_lossy()).standardized_file_url();
    let (text, _) = DocumentIO::read(Path::new(&url.path())).map_err(|error| Failure::Error(error.to_string()))?;
    let mut exporter = HTMLExporter::new(
        MarkdownParser::parse(&text),
        style_sheet.theme.clone(),
        url.deleting_path_extension().last_path_component(),
        Some(url.deleting_last_path_component()),
        Some(Box::new(NativeFragmentImageProvider::new(style_sheet))),
    );
    exporter.for_print = flags.print;
    let result = RefCell::new(String::new());
    let block = RcBlock::new(|| {
        *result.borrow_mut() = exporter.html();
    });
    appearance.performAsCurrentDrawingAppearance(&block);
    drop(block);
    Ok(result.into_inner())
}

pub fn run(request: &Request) -> Result<(), Failure> {
    let html = html(&request.input, &flags(&request.flags)?)?;
    let lines: Vec<Value> = upleft_swift_text::components_separated_by(&html, "\n").into_iter().map(Value::String).collect();
    json::write(&Object::new().with("lines", lines).build(), &request.output)?;
    Ok(())
}
