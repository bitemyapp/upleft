//! Rust counterparts of the Swift oracle's dumps (`oracle/Sources/downright-oracle`).
//! Each submodule mirrors one Swift file and must emit the same JSON shape.

pub mod core_text;
pub mod attribute_dump;
pub mod decorate;
pub mod display_map;
pub mod highlight;
pub mod incremental;
pub mod json;
pub mod markup;
pub mod math;
pub mod math_bench;
pub mod parse;
pub mod render;
pub mod view_bench;
pub mod style_sheet;
pub mod unicode;

use std::path::PathBuf;

/// The flags both oracles accept.
#[derive(Debug, Clone)]
pub struct Request {
    pub command: String,
    pub input: PathBuf,
    pub output: PathBuf,
    /// `read`, `live`, or `source`.
    pub mode: String,
    pub theme: String,
    pub dark: bool,
    pub width: f64,
    pub height: f64,
    pub layout: Option<PathBuf>,
    /// Capture the composited window (default) rather than `cacheDisplay`.
    pub capture_from_screen: bool,
}

impl Request {
    pub fn parse(arguments: &[String]) -> Result<Request, String> {
        let [command, input, output, flags @ ..] = arguments else {
            return Err("expected <command> <input> <output>".into());
        };
        let mut request = Request {
            command: command.clone(),
            input: input.into(),
            output: output.into(),
            mode: "live".into(),
            theme: "Paper Light".into(),
            dark: false,
            width: 1000.0,
            height: 1400.0,
            layout: None,
            capture_from_screen: true,
        };
        let mut flags = flags.iter();
        while let Some(flag) = flags.next() {
            let mut value = || flags.next().cloned().ok_or_else(|| format!("{flag} needs a value"));
            match flag.as_str() {
                "--mode" => {
                    let mode = value()?;
                    if !matches!(mode.as_str(), "read" | "live" | "source") {
                        return Err(format!("unknown mode {mode}"));
                    }
                    request.mode = mode;
                }
                "--theme" => request.theme = value()?,
                "--dark" => request.dark = true,
                "--width" => request.width = value()?.parse().map_err(|_| "--width takes a number")?,
                "--height" => request.height = value()?.parse().map_err(|_| "--height takes a number")?,
                "--layout" => request.layout = Some(value()?.into()),
                "--capture" => {
                    request.capture_from_screen = match value()?.as_str() {
                        "screen" => true,
                        "view" => false,
                        other => return Err(format!("unknown capture {other}")),
                    }
                }
                other => return Err(format!("unknown flag {other}")),
            }
        }
        Ok(request)
    }
}

#[derive(Debug)]
pub enum Failure {
    /// The layer this command exercises has not been ported yet.
    NotPorted,
    Error(String),
}

impl From<std::io::Error> for Failure {
    fn from(error: std::io::Error) -> Self {
        Failure::Error(error.to_string())
    }
}

/// Dispatches a request. Ported layers add their command here.
pub fn run(request: &Request) -> Result<(), Failure> {
    match request.command.as_str() {
        "math" => math::image(&request.input, &request.output, &request.theme, request.dark),
        "math-tree" => math::tree(&request.input, &request.output, &request.theme, request.dark),
        "bench-math" => math_bench::run(&request.input, &request.output),
        "markup" => markup::run(&request.input, &request.output),
        "parse" => parse::run(&request.input, &request.output),
        "core-text" => core_text::run(&request.input, &request.output),
        "unicode" => unicode::run(&request.input, &request.output),
        "bench-core-text" => core_text::bench(&request.input, &request.output),
        "decorate" => decorate::run(request),
        "incremental" => incremental::run(request),
        "displaymap" => display_map::run(request),
        "stylesheet" => {
            let value = style_sheet::dump(&request.theme, request.dark)?;
            Ok(json::write(&value, &request.output)?)
        }
        "highlight" => {
            let text = std::fs::read_to_string(&request.input)?;
            Ok(json::write(&highlight::document(&text), &request.output)?)
        }
        "vscode-theme" => {
            let data = std::fs::read(&request.input)?;
            Ok(json::write(&highlight::vscode_theme(&data, &request.input), &request.output)?)
        }
        "probe" => crate::capture::run(request.capture(), Box::new(crate::capture::ProbeScene)),
        "bench-view" => view_bench::run(request),
        "render" => crate::capture::run(
            request.capture(),
            Box::new(render::MarkdownScene::new(&request.mode, &request.theme)),
        ),
        _ => Err(Failure::NotPorted),
    }
}

impl Request {
    /// The window-capture parameters shared by `render` and `probe`.
    pub fn capture(&self) -> crate::capture::CaptureRequest {
        crate::capture::CaptureRequest {
            input: self.input.clone(),
            output_png: self.output.clone(),
            output_layout: self.layout.clone(),
            dark: self.dark,
            width: self.width,
            height: self.height,
            settle_timeout: std::time::Duration::from_secs(8),
            capture_from_screen: self.capture_from_screen,
        }
    }
}
