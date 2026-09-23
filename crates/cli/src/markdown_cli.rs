//! Port of `Sources/drdownright/MarkdownCLI.swift`: the command-line surface
//! shared by `down` and the `md` alias.
//!
//! Parsing and pure document operations live in a library so they can be
//! exercised without launching an app or touching the user's files.
//!
//! String handling follows Swift exactly: `hasPrefix`, `count`, `dropFirst`
//! and `split` walk Characters (`upleft-swift-text`), `replacingOccurrences`
//! and `components(separatedBy:)` are Foundation's, and the regular
//! expressions run on `NSRegularExpression` itself.

use objc2::rc::{Retained, autoreleasepool};
use objc2_foundation::{
    NSMatchingOptions, NSRange as FRange, NSRegularExpression, NSRegularExpressionOptions, NSString,
    NSStringCompareOptions,
};
use upleft_core::ParseOptions;
use upleft_core::compatibility::compatibility_diagnostics::{CompatibilityDiagnostic, MarkdownCompatibility};
use upleft_core::compatibility::render_target::BuiltInRenderTarget;
use upleft_core::health::document_health::{
    DocumentHealth, DocumentHealthDiagnostic, DocumentHealthOptions, DocumentHealthResolver,
};
use upleft_core::parser::MarkdownParser;
use upleft_foundation::foundation_io::{self, FoundationError, cocoa_code};
use upleft_foundation::json_serialization::{self, AnyJson, ReadingOptions};
use upleft_foundation::url::FileUrl;
use upleft_swift_text::{self as swift_text, CharSet};

use crate::agent_bridge::HookScope;
use crate::agent_watcher::AgentWatcher;

/// `MarkdownCLI.version`.
pub const VERSION: &str = "1.0.16";

/// Settings are user-owned configuration. Bound reads so a special file or
/// unexpectedly large document cannot make `down hook` consume unbounded
/// memory, and never turn a read failure into an empty configuration.
pub const MAXIMUM_SETTINGS_BYTES: usize = 1_048_576;

/// `MarkdownCLI.SettingsFileError`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SettingsFileError {
    Unreadable { path: String, reason: String },
    TooLarge { path: String, maximum_bytes: isize },
    InvalidJson { path: String },
}

impl std::fmt::Display for SettingsFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SettingsFileError::Unreadable { path, reason } => write!(f, "cannot read {path}: {reason}"),
            SettingsFileError::TooLarge { path, maximum_bytes } => {
                write!(f, "cannot read {path}: settings exceed {maximum_bytes} bytes")
            }
            SettingsFileError::InvalidJson { path } => write!(f, "cannot read {path}: settings must be a JSON object"),
        }
    }
}

impl std::error::Error for SettingsFileError {}

/// `MarkdownCLI.Action`.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Open(OpenOptions, Vec<String>),
    Read { json: bool, paths: Vec<String> },
    Export { format: ExportFormat, output: Option<String>, paths: Vec<String> },
    Check { json: bool, target: Option<BuiltInRenderTarget>, paths: Vec<String> },
    Outline { json: bool, paths: Vec<String> },
    Doctor { json: bool, app_path: Option<String> },
    Notify(NotifyOptions),
    Watch(WatchOptions, Vec<String>),
    Hook(HookOptions),
    Help,
    Version,
}

/// `MarkdownCLI.OpenOptions`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OpenOptions {
    pub new_window: bool,
    pub background: bool,
    pub wait: bool,
    pub edit: bool,
    pub line: Option<isize>,
    pub reveal: bool,
    pub review: bool,
}

/// `down notify` — the agent hook endpoint. Reads a hook payload on stdin
/// and hands any Markdown file it names to Downright.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NotifyOptions {
    /// Bring Downright to the front. Off by default: the whole point is to
    /// queue a change for review without pulling focus out of the terminal
    /// the agent is running in.
    pub focus: bool,
    /// Print the resolved paths instead of opening them, for wiring up a
    /// hook without launching anything.
    pub dry_run: bool,
}

/// `down watch` — the no-hook fallback for agents with no hook system.
#[derive(Clone, Debug, PartialEq)]
pub struct WatchOptions {
    pub focus: bool,
    pub debounce: f64,
}

impl Default for WatchOptions {
    fn default() -> Self {
        WatchOptions { focus: false, debounce: AgentWatcher::DEFAULT_DEBOUNCE }
    }
}

/// `MarkdownCLI.HookOptions.Mode`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HookMode {
    #[default]
    Print,
    Install,
    Uninstall,
}

impl HookMode {
    pub fn raw_value(&self) -> &'static str {
        match self {
            HookMode::Print => "print",
            HookMode::Install => "install",
            HookMode::Uninstall => "uninstall",
        }
    }
}

/// `down hook` — print or install the agent configuration that wires
/// `down notify` in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HookOptions {
    pub mode: HookMode,
    pub scope: HookScope,
}

impl Default for HookOptions {
    fn default() -> Self {
        HookOptions { mode: HookMode::Print, scope: HookScope::Project }
    }
}

/// `MarkdownCLI.ExportFormat`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportFormat {
    Html,
}

impl ExportFormat {
    pub const ALL_CASES: [ExportFormat; 1] = [ExportFormat::Html];

    pub fn raw_value(&self) -> &'static str {
        "html"
    }

    /// `init?(argument:)`: `rawValue` matched against the lowercased argument.
    pub fn from_argument(argument: &str) -> Option<ExportFormat> {
        let lowered = swift_text::lowercased(argument);
        ExportFormat::ALL_CASES.into_iter().find(|format| swift_text::str_eq(format.raw_value(), &lowered))
    }
}

/// `MarkdownCLI.ParseError`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseError {
    UnknownOption(String),
    MissingValue(String),
    InvalidFormat(String),
    InvalidLine(String),
    UnexpectedArgument(String),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::UnknownOption(value) => write!(f, "unknown option {value}"),
            ParseError::MissingValue(value) => write!(f, "missing value for {value}"),
            ParseError::InvalidFormat(value) => write!(f, "unsupported export format {value}"),
            ParseError::InvalidLine(value) => write!(f, "line must be a positive integer, got {value}"),
            ParseError::UnexpectedArgument(value) => write!(f, "unexpected argument {value}"),
        }
    }
}

impl std::error::Error for ParseError {}

/// A Swift `switch` case on a `String`: `==`, canonical equivalence.
fn is(argument: &str, literal: &str) -> bool {
    swift_text::str_eq(argument, literal)
}

fn is_any(argument: &str, literals: &[&str]) -> bool {
    literals.iter().any(|literal| is(argument, literal))
}

/// `MarkdownCLI.parse(_:)`.
pub fn parse(arguments: &[String]) -> Result<Action, ParseError> {
    let Some(first) = arguments.first() else { return Ok(Action::Open(OpenOptions::default(), Vec::new())) };
    if is(first, "-h") || is(first, "--help") {
        return Ok(Action::Help);
    }
    if is(first, "-v") || is(first, "--version") {
        return Ok(Action::Version);
    }
    let rest = &arguments[1..];
    if is(first, "read") {
        parse_read(rest)
    } else if is(first, "export") {
        parse_export(rest)
    } else if is(first, "check") {
        parse_check(rest)
    } else if is(first, "outline") {
        parse_outline(rest)
    } else if is(first, "doctor") {
        parse_doctor(rest)
    } else if is(first, "open") {
        parse_open(rest)
    } else if is(first, "notify") {
        parse_notify(rest)
    } else if is(first, "watch") {
        parse_watch(rest)
    } else if is(first, "hook") {
        parse_hook(rest)
    } else {
        parse_open(arguments)
    }
}

/// `MarkdownCLI.usage()`.
pub fn usage() -> String {
    format!(
        "down {VERSION} — open and inspect Markdown in Downright

USAGE
  down [open options] [file ...]
  down read [--json] [file ...]
  down export [--format html] [-o path] [file ...]
  down check [--json] [--target name] [file or folder ...]
  down outline [--json] [file ...]
  down doctor [--json] [--app path]
  down notify [--focus] [--dry-run]
  down watch [--focus] [--debounce ms] [file or folder ...]
  down hook [--print | --install | --uninstall] [--scope user|project]
  … | down [command] -

COMMANDS
  open       Open files in Downright (the default)
  read       Write Markdown source to stdout
  export     Write self-contained HTML to stdout or -o a file
  check      Run health and target checks (exit 1 when findings exist)
  outline    List document headings
  notify     Open the Markdown file named by an agent hook payload on
             stdin.  Always exits 0 so a hook never blocks the agent.
  watch      Open Markdown files in the background as they change.  The
             fallback for agents that have no hook system.
  hook       Print or install the agent configuration that runs `notify`

CHECK TARGETS
  downright, commonmark, github, obsidian, pandoc, multimarkdown,
  jekyll, hugo, quarto

OPEN OPTIONS
  -n, --new         open each file in a new window
  -b, --background  do not bring Downright to the front
  -w, --wait        wait for the app to exit
  -e, --edit        open in Live mode instead of Read mode
  --line N          open at one-based line N
  --reveal          reveal the file in Finder instead of opening it
  --review          open in Live mode with the Review panel visible
  -h, --help        show this message
  -v, --version     show the version

AGENT OPTIONS
  --focus           bring Downright forward (default: stay in background)
  --dry-run         print what notify would open, without opening it
  --debounce ms     quiet period before reporting a burst (default 300)
  --scope           where `hook --install` writes: user or project

AGENT SETUP
  down hook --install            wire this project's agent to Downright
  down hook --install --scope user   wire every project for this user"
    )
}

/// `argument.hasPrefix("-") && argument.count > 1`.
fn looks_like_option(argument: &str) -> bool {
    swift_text::has_prefix(argument, "-") && swift_text::count(argument) > 1
}

/// `argument != "-", argument.hasPrefix("-")`.
fn is_option_not_stdin(argument: &str) -> bool {
    !is(argument, "-") && swift_text::has_prefix(argument, "-")
}

fn parse_open(arguments: &[String]) -> Result<Action, ParseError> {
    let mut options = OpenOptions::default();
    let mut paths = Vec::new();
    let mut index = 0;
    while index < arguments.len() {
        let argument = &arguments[index];
        if is(argument, "--") {
            paths.extend_from_slice(&arguments[index + 1..]);
            index = arguments.len();
            continue;
        } else if is_any(argument, &["-n", "--new"]) {
            options.new_window = true;
        } else if is_any(argument, &["-b", "--background"]) {
            options.background = true;
        } else if is_any(argument, &["-w", "--wait"]) {
            options.wait = true;
        } else if is_any(argument, &["-e", "--edit"]) {
            options.edit = true;
        } else if is(argument, "--line") {
            index += 1;
            if index >= arguments.len() {
                return Err(ParseError::MissingValue(argument.clone()));
            }
            let value = &arguments[index];
            match swift_text::parse_int(value) {
                Some(line) if line > 0 => options.line = Some(line),
                _ => return Err(ParseError::InvalidLine(value.clone())),
            }
        } else if is(argument, "--reveal") {
            options.reveal = true;
        } else if is(argument, "--review") {
            options.review = true;
            options.edit = true;
        } else if is_any(argument, &["-h", "--help"]) {
            return Ok(Action::Help);
        } else if is_any(argument, &["-v", "--version"]) {
            return Ok(Action::Version);
        } else {
            if looks_like_option(argument) {
                return Err(ParseError::UnknownOption(argument.clone()));
            }
            paths.push(argument.clone());
        }
        index += 1;
    }
    Ok(Action::Open(options, paths))
}

fn parse_doctor(arguments: &[String]) -> Result<Action, ParseError> {
    let mut json = false;
    let mut app_path = None;
    let mut index = 0;
    while index < arguments.len() {
        let argument = &arguments[index];
        if is(argument, "--json") {
            json = true;
        } else if is(argument, "--app") {
            index += 1;
            if index >= arguments.len() {
                return Err(ParseError::MissingValue("--app".into()));
            }
            app_path = Some(arguments[index].clone());
        } else if is_any(argument, &["-h", "--help"]) {
            return Ok(Action::Help);
        } else if is_any(argument, &["-v", "--version"]) {
            return Ok(Action::Version);
        } else {
            return Err(ParseError::UnknownOption(argument.clone()));
        }
        index += 1;
    }
    Ok(Action::Doctor { json, app_path })
}

fn parse_read(arguments: &[String]) -> Result<Action, ParseError> {
    let mut json = false;
    let mut paths = Vec::new();
    let mut parsing_options = true;
    for argument in arguments {
        if parsing_options && is(argument, "--") {
            parsing_options = false;
            continue;
        }
        if !parsing_options {
            paths.push(argument.clone());
            continue;
        }
        if is(argument, "--json") {
            json = true;
        } else if is_any(argument, &["-h", "--help"]) {
            return Ok(Action::Help);
        } else if is_any(argument, &["-v", "--version"]) {
            return Ok(Action::Version);
        } else {
            if is_option_not_stdin(argument) {
                return Err(ParseError::UnknownOption(argument.clone()));
            }
            paths.push(argument.clone());
        }
    }
    Ok(Action::Read { json, paths })
}

fn parse_check(arguments: &[String]) -> Result<Action, ParseError> {
    let mut json = false;
    let mut target = None;
    let mut paths = Vec::new();
    let mut index = 0;
    while index < arguments.len() {
        let argument = &arguments[index];
        if is(argument, "--json") {
            json = true;
        } else if is(argument, "--target") {
            index += 1;
            if index >= arguments.len() {
                return Err(ParseError::MissingValue(argument.clone()));
            }
            let Some(value) = render_target(&arguments[index]) else {
                return Err(ParseError::UnexpectedArgument(format!("unknown render target {}", arguments[index])));
            };
            target = Some(value);
        } else if is(argument, "--") {
            paths.extend_from_slice(&arguments[index + 1..]);
            index = arguments.len();
        } else if is_any(argument, &["-h", "--help"]) {
            return Ok(Action::Help);
        } else if is_any(argument, &["-v", "--version"]) {
            return Ok(Action::Version);
        } else {
            if is_option_not_stdin(argument) {
                return Err(ParseError::UnknownOption(argument.clone()));
            }
            paths.push(argument.clone());
        }
        index += 1;
    }
    Ok(Action::Check { json, target, paths })
}

fn parse_outline(arguments: &[String]) -> Result<Action, ParseError> {
    let mut json = false;
    let mut paths = Vec::new();
    let mut index = 0;
    while index < arguments.len() {
        let argument = &arguments[index];
        if is(argument, "--json") {
            json = true;
        } else if is(argument, "--") {
            paths.extend_from_slice(&arguments[index + 1..]);
            index = arguments.len();
        } else if is_any(argument, &["-h", "--help"]) {
            return Ok(Action::Help);
        } else if is_any(argument, &["-v", "--version"]) {
            return Ok(Action::Version);
        } else {
            if is_option_not_stdin(argument) {
                return Err(ParseError::UnknownOption(argument.clone()));
            }
            paths.push(argument.clone());
        }
        index += 1;
    }
    Ok(Action::Outline { json, paths })
}

fn parse_notify(arguments: &[String]) -> Result<Action, ParseError> {
    let mut options = NotifyOptions::default();
    for argument in arguments {
        if is(argument, "--focus") {
            options.focus = true;
        } else if is(argument, "--dry-run") {
            options.dry_run = true;
        } else if is_any(argument, &["-h", "--help"]) {
            return Ok(Action::Help);
        } else if is_any(argument, &["-v", "--version"]) {
            return Ok(Action::Version);
        } else {
            return Err(ParseError::UnknownOption(argument.clone()));
        }
    }
    Ok(Action::Notify(options))
}

fn parse_watch(arguments: &[String]) -> Result<Action, ParseError> {
    let mut options = WatchOptions::default();
    let mut paths = Vec::new();
    let mut index = 0;
    while index < arguments.len() {
        let argument = &arguments[index];
        if is(argument, "--focus") {
            options.focus = true;
        } else if is(argument, "--debounce") {
            index += 1;
            if index >= arguments.len() {
                return Err(ParseError::MissingValue(argument.clone()));
            }
            match swift_double(&arguments[index]) {
                Some(milliseconds) if milliseconds >= 0.0 => options.debounce = milliseconds / 1000.0,
                _ => {
                    return Err(ParseError::UnexpectedArgument(format!(
                        "--debounce expects milliseconds, got {}",
                        arguments[index]
                    )));
                }
            }
        } else if is(argument, "--") {
            paths.extend_from_slice(&arguments[index + 1..]);
            index = arguments.len();
        } else if is_any(argument, &["-h", "--help"]) {
            return Ok(Action::Help);
        } else if is_any(argument, &["-v", "--version"]) {
            return Ok(Action::Version);
        } else {
            if looks_like_option(argument) {
                return Err(ParseError::UnknownOption(argument.clone()));
            }
            paths.push(argument.clone());
        }
        index += 1;
    }
    Ok(Action::Watch(options, paths))
}

fn parse_hook(arguments: &[String]) -> Result<Action, ParseError> {
    let mut options = HookOptions::default();
    let mut index = 0;
    while index < arguments.len() {
        let argument = &arguments[index];
        if is(argument, "--print") {
            options.mode = HookMode::Print;
        } else if is(argument, "--install") {
            options.mode = HookMode::Install;
        } else if is(argument, "--uninstall") {
            options.mode = HookMode::Uninstall;
        } else if is(argument, "--scope") {
            index += 1;
            if index >= arguments.len() {
                return Err(ParseError::MissingValue(argument.clone()));
            }
            let Some(scope) = HookScope::from_raw_value(&swift_text::lowercased(&arguments[index])) else {
                return Err(ParseError::UnexpectedArgument(format!(
                    "unknown scope {}; expected user or project",
                    arguments[index]
                )));
            };
            options.scope = scope;
        } else if is_any(argument, &["-h", "--help"]) {
            return Ok(Action::Help);
        } else if is_any(argument, &["-v", "--version"]) {
            return Ok(Action::Version);
        } else {
            return Err(ParseError::UnknownOption(argument.clone()));
        }
        index += 1;
    }
    Ok(Action::Hook(options))
}

fn parse_export(arguments: &[String]) -> Result<Action, ParseError> {
    let mut format = ExportFormat::Html;
    let mut output = None;
    let mut paths = Vec::new();
    let mut index = 0;
    while index < arguments.len() {
        let argument = &arguments[index];
        if is_any(argument, &["--format", "-f"]) {
            index += 1;
            if index >= arguments.len() {
                return Err(ParseError::MissingValue(argument.clone()));
            }
            let Some(value) = ExportFormat::from_argument(&arguments[index]) else {
                return Err(ParseError::InvalidFormat(arguments[index].clone()));
            };
            format = value;
        } else if is_any(argument, &["--output", "-o"]) {
            index += 1;
            if index >= arguments.len() {
                return Err(ParseError::MissingValue(argument.clone()));
            }
            output = Some(arguments[index].clone());
        } else if is(argument, "--") {
            paths.extend_from_slice(&arguments[index + 1..]);
            index = arguments.len();
        } else if is_any(argument, &["-h", "--help"]) {
            return Ok(Action::Help);
        } else if is_any(argument, &["-v", "--version"]) {
            return Ok(Action::Version);
        } else {
            if is_option_not_stdin(argument) {
                return Err(ParseError::UnknownOption(argument.clone()));
            }
            paths.push(argument.clone());
        }
        index += 1;
    }
    Ok(Action::Export { format, output, paths })
}

/// Swift's `Double(_: String)` (Swift 6.4): the whole text must be a decimal
/// or hexadecimal floating-point literal, `inf`/`infinity` or `nan` (any
/// case, optionally signed); no surrounding whitespace. Out-of-range values
/// become `±inf` or `±0` rather than failing. Only whether the result exists
/// and is `>= 0` is observable in `down`, so NaN payloads are not modelled.
pub fn swift_double(text: &str) -> Option<f64> {
    let (negative, body) = match text.as_bytes().first()? {
        b'+' => (false, &text[1..]),
        b'-' => (true, &text[1..]),
        _ => (false, text),
    };
    let lower = body.to_ascii_lowercase();
    if lower == "inf" || lower == "infinity" {
        return Some(if negative { f64::NEG_INFINITY } else { f64::INFINITY });
    }
    if lower == "nan" || lower == "snan" || (lower.starts_with("nan(") && lower.ends_with(')')) {
        return Some(f64::NAN);
    }
    let value = if let Some(hex) = lower.strip_prefix("0x") { parse_hex_float(hex)? } else { parse_decimal_float(&lower)? };
    Some(if negative { -value } else { value })
}

fn parse_decimal_float(body: &str) -> Option<f64> {
    let bytes = body.as_bytes();
    let digits_before = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
    let mut index = digits_before;
    let mut digits_after = 0;
    if bytes.get(index) == Some(&b'.') {
        index += 1;
        digits_after = bytes[index..].iter().take_while(|b| b.is_ascii_digit()).count();
        index += digits_after;
    }
    if digits_before + digits_after == 0 {
        return None;
    }
    if bytes.get(index) == Some(&b'e') {
        index += 1;
        if matches!(bytes.get(index), Some(b'+') | Some(b'-')) {
            index += 1;
        }
        let exponent_digits = bytes[index..].iter().take_while(|b| b.is_ascii_digit()).count();
        if exponent_digits == 0 {
            return None;
        }
        index += exponent_digits;
    }
    if index != bytes.len() {
        return None;
    }
    // Rust's parser rounds correctly and saturates to inf / 0 as Swift's does.
    body.parse::<f64>().ok()
}

fn parse_hex_float(body: &str) -> Option<f64> {
    let bytes = body.as_bytes();
    let mut index = 0;
    let mut mantissa: f64 = 0.0;
    let mut any_digit = false;
    let mut exponent: i64 = 0;
    while let Some(digit) = bytes.get(index).and_then(|b| (*b as char).to_digit(16)) {
        mantissa = mantissa * 16.0 + digit as f64;
        any_digit = true;
        index += 1;
    }
    if bytes.get(index) == Some(&b'.') {
        index += 1;
        while let Some(digit) = bytes.get(index).and_then(|b| (*b as char).to_digit(16)) {
            mantissa = mantissa * 16.0 + digit as f64;
            exponent -= 4;
            any_digit = true;
            index += 1;
        }
    }
    if !any_digit {
        return None;
    }
    if bytes.get(index) == Some(&b'p') {
        index += 1;
        let negative = match bytes.get(index) {
            Some(b'+') => {
                index += 1;
                false
            }
            Some(b'-') => {
                index += 1;
                true
            }
            _ => false,
        };
        let start = index;
        let mut value: i64 = 0;
        while let Some(b) = bytes.get(index).filter(|b| b.is_ascii_digit()) {
            value = value.saturating_mul(10).saturating_add((b - b'0') as i64);
            index += 1;
        }
        if index == start {
            return None;
        }
        exponent = exponent.saturating_add(if negative { -value } else { value });
    }
    if index != bytes.len() {
        return None;
    }
    Some(mantissa * 2f64.powi(exponent.clamp(-10_000, 10_000) as i32))
}

// MARK: - HTML export

/// A small, deterministic HTML writer for terminal exports. It deliberately
/// emits no external resources, scripts, or network references.
///
/// `MarkdownCLI.html(for:title:)`; Swift's default title is `"Markdown"`.
pub fn html(markdown: &str, title: &str) -> String {
    let lines = swift_text::components_separated_by_set(markdown, CharSet::Newlines);
    let mut body: Vec<String> = Vec::new();
    let mut paragraph: Vec<String> = Vec::new();
    let mut in_code = false;
    let mut code_language = String::new();
    let mut code_lines: Vec<String> = Vec::new();

    fn flush_paragraph(paragraph: &mut Vec<String>, body: &mut Vec<String>) {
        if paragraph.is_empty() {
            return;
        }
        body.push(format!("<p>{}</p>", inline(&paragraph.join("\n"))));
        paragraph.clear();
    }
    let mut self_list_kind: Option<char> = None;
    fn close_list(self_list_kind: &mut Option<char>, body: &mut Vec<String>) {
        let Some(list_kind) = *self_list_kind else { return };
        body.push(if list_kind == '1' { "</ol>".into() } else { "</ul>".into() });
        *self_list_kind = None;
    }
    for line in &lines {
        if in_code {
            if swift_text::has_prefix(line, "```") {
                body.push(format!(
                    "<pre><code class=\"language-{}\">{}</code></pre>",
                    escape(&code_language),
                    escape(&code_lines.join("\n"))
                ));
                code_lines.clear();
                in_code = false;
            } else {
                code_lines.push(line.clone());
            }
            continue;
        }
        if swift_text::has_prefix(line, "```") {
            flush_paragraph(&mut paragraph, &mut body);
            close_list(&mut self_list_kind, &mut body);
            in_code = true;
            code_language = swift_text::trimming(swift_text::drop_first(line, 3), CharSet::Whitespaces).to_owned();
            continue;
        }
        if swift_text::trimming(line, CharSet::Whitespaces).is_empty() {
            flush_paragraph(&mut paragraph, &mut body);
            close_list(&mut self_list_kind, &mut body);
            continue;
        }
        if let Some((level, text)) = heading(line) {
            flush_paragraph(&mut paragraph, &mut body);
            close_list(&mut self_list_kind, &mut body);
            body.push(format!("<h{level}>{}</h{level}>", inline(text)));
            continue;
        }
        if swift_text::has_prefix(line, "> ") {
            flush_paragraph(&mut paragraph, &mut body);
            close_list(&mut self_list_kind, &mut body);
            body.push(format!("<blockquote>{}</blockquote>", inline(swift_text::drop_first(line, 2))));
            continue;
        }
        if is(line, "---") || is(line, "***") {
            flush_paragraph(&mut paragraph, &mut body);
            close_list(&mut self_list_kind, &mut body);
            body.push("<hr>".into());
            continue;
        }
        if let Some(item) = list_item(line) {
            flush_paragraph(&mut paragraph, &mut body);
            let wanted = if item.ordered { '1' } else { 'u' };
            if self_list_kind != Some(wanted) {
                close_list(&mut self_list_kind, &mut body);
                body.push(if item.ordered { "<ol>".into() } else { "<ul>".into() });
                self_list_kind = Some(wanted);
            }
            let checkbox = match item.checkbox {
                Some(checked) => format!("<input type=\"checkbox\" disabled{}> ", if checked { " checked" } else { "" }),
                None => String::new(),
            };
            body.push(format!("<li>{checkbox}{}</li>", inline(&item.text)));
            continue;
        }
        paragraph.push(line.clone());
    }
    if in_code {
        body.push(format!("<pre><code>{}</code></pre>", escape(&code_lines.join("\n"))));
    }
    flush_paragraph(&mut paragraph, &mut body);
    close_list(&mut self_list_kind, &mut body);
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{}</title><style>body{{font:16px/1.55 -apple-system,BlinkMacSystemFont,sans-serif;max-width:70ch;margin:3rem auto;padding:0 1rem;color:#222}}pre{{padding:1rem;background:#f3f3f3;overflow:auto}}code{{font-family:ui-monospace,monospace}}blockquote{{border-left:3px solid #aaa;padding-left:1rem;color:#555}}img{{max-width:100%}}</style></head><body>{}</body></html>",
        escape(title),
        body.join("\n")
    )
}

/// `MarkdownCLI.diagnostics(for:baseURL:)`.
pub fn diagnostics(markdown: &str, base_url: Option<&FileUrl>) -> Vec<DocumentHealthDiagnostic> {
    let resolver = base_url.map(|base| {
        let base = base.clone();
        DocumentHealthResolver::new(move |path| {
            foundation_io::file_exists(&FileUrl::from_path_relative_to(path, &base).standardized_file_url().path())
        })
    });
    DocumentHealth::analyze_with(markdown, DocumentHealthOptions::DEFAULT, resolver.as_ref())
}

/// `MarkdownCLI.OutlineItem`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutlineItem {
    pub level: isize,
    pub title: String,
    pub slug: String,
    pub location: isize,
    pub line: isize,
}

impl OutlineItem {
    /// The synthesized `Codable` encoding, members in `CodingKeys` order.
    pub fn json_value(&self) -> upleft_foundation::json_encoder::JsonValue {
        use upleft_foundation::json_encoder::JsonValue;
        JsonValue::object([
            ("level", JsonValue::Int(self.level as i64)),
            ("title", JsonValue::String(self.title.clone())),
            ("slug", JsonValue::String(self.slug.clone())),
            ("location", JsonValue::Int(self.location as i64)),
            ("line", JsonValue::Int(self.line as i64)),
        ])
    }
}

/// `MarkdownCLI.outline(for:)`.
pub fn outline(markdown: &str) -> Vec<OutlineItem> {
    let parsed = MarkdownParser::parse_with(markdown, ParseOptions::STRUCTURE_ONLY);
    parsed
        .headings
        .iter()
        .map(|heading| OutlineItem {
            level: heading.level,
            title: heading.title.clone(),
            slug: heading.slug.clone(),
            location: heading.range.location,
            line: parsed.line_at(heading.range.location),
        })
        .collect()
}

/// `MarkdownCLI.compatibilityDiagnostics(for:target:)`.
pub fn compatibility_diagnostics(markdown: &str, target: BuiltInRenderTarget) -> Vec<CompatibilityDiagnostic> {
    MarkdownCompatibility::diagnose(&MarkdownParser::parse(markdown), &target.profile()).diagnostics
}

/// `MarkdownCLI.renderTarget(named:)`.
pub fn render_target(name: &str) -> Option<BuiltInRenderTarget> {
    let normalized = swift_text::replacing_occurrences(&swift_text::lowercased(name), "-", "");
    BuiltInRenderTarget::ALL_CASES.into_iter().find(|target| {
        is(&swift_text::lowercased(target.raw_value()), &normalized)
            || is(&swift_text::replacing_occurrences(&swift_text::lowercased(target.display_name()), "-", ""), &normalized)
    })
}

/// The extensions `isMarkdownPath` accepts.
pub const MARKDOWN_EXTENSIONS: [&str; 8] = ["md", "markdown", "mdown", "mkd", "mdx", "mdc", "qmd", "rmd"];

/// `MarkdownCLI.isMarkdownPath(_:)`.
pub fn is_markdown_path(path: &str) -> bool {
    let extension = swift_text::lowercased(&FileUrl::from_path(path).path_extension());
    MARKDOWN_EXTENSIONS.iter().any(|supported| is(supported, &extension))
}

/// Loads a hook settings object without ever treating damaged input as an
/// absent file. The caller may safely write only after this returns.
///
/// `MarkdownCLI.loadSettings(at:maximumBytes:)`; the result is always an
/// [`AnyJson::Object`].
pub fn load_settings(url: &FileUrl, maximum_bytes: isize) -> Result<AnyJson, SettingsFileError> {
    let path = url.path();
    if maximum_bytes < 0 {
        return Err(SettingsFileError::Unreadable { path, reason: "maximum byte count must not be negative".into() });
    }
    let attributes = match foundation_io::attributes_of_item(&path) {
        Ok(attributes) => attributes,
        Err(error) if error.is_cocoa(cocoa_code::FILE_READ_NO_SUCH_FILE) => return Ok(AnyJson::Object(Vec::new())),
        Err(error) => return Err(SettingsFileError::Unreadable { path, reason: error.description }),
    };
    if attributes.file_type.as_deref() != Some(foundation_io::file_type_regular().as_str()) {
        return Err(SettingsFileError::Unreadable { path, reason: "not a regular file".into() });
    }
    if let Some(size) = attributes.size
        && size > maximum_bytes as u64
    {
        return Err(SettingsFileError::TooLarge { path, maximum_bytes });
    }
    let data = match foundation_io::read_up_to_count(url, maximum_bytes as usize + 1) {
        Ok(data) => data,
        Err(FoundationError { description, .. }) => {
            return Err(SettingsFileError::Unreadable { path, reason: description });
        }
    };
    if data.len() > maximum_bytes as usize {
        return Err(SettingsFileError::TooLarge { path, maximum_bytes });
    }
    match json_serialization::json_object(&data, ReadingOptions::default()) {
        Ok(AnyJson::Object(members)) => Ok(AnyJson::Object(crate::agent_bridge::bridged(&members))),
        _ => Err(SettingsFileError::InvalidJson { path }),
    }
}

/// `MarkdownCLI.loadSettings(at:)` with the default limit.
pub fn load_settings_default(url: &FileUrl) -> Result<AnyJson, SettingsFileError> {
    load_settings(url, MAXIMUM_SETTINGS_BYTES as isize)
}

fn escape(value: &str) -> String {
    let value = swift_text::replacing_occurrences(value, "&", "&amp;");
    let value = swift_text::replacing_occurrences(&value, "<", "&lt;");
    let value = swift_text::replacing_occurrences(&value, ">", "&gt;");
    swift_text::replacing_occurrences(&value, "\"", "&quot;")
}

fn inline(value: &str) -> String {
    let mut result = escape(value);
    result = regex_replace(&result, r"`([^`]+)`", "<code>$1</code>");
    result = regex_replace(&result, r"\*\*([^*]+)\*\*", "<strong>$1</strong>");
    result = regex_replace(&result, r"\*([^*]+)\*", "<em>$1</em>");
    result = replacing_matches(&result, r"!\[([^\]]*)\]\(([^)]+)\)", |captures| {
        let alt = &captures[0];
        let source = removing_url_line_breaks(&captures[1]);
        if !image_source_is_safe(&source) {
            return format!("<span class=\"missing-image\" title=\"{source}\">{alt}</span>");
        }
        format!("<img alt=\"{alt}\" src=\"{source}\">")
    });
    result = replacing_matches(&result, r"\[([^\]]+)\]\(([^)]+)\)", |captures| {
        let label = &captures[0];
        let destination = removing_url_line_breaks(&captures[1]);
        if !link_destination_is_safe(&destination) {
            return format!("<span title=\"{destination}\">{label}</span>");
        }
        format!("<a href=\"{destination}\">{label}</a>")
    });
    swift_text::replacing_occurrences(&result, "\n", "<br>\n")
}

const LINK_SCHEME_ALLOWLIST: [&str; 3] = ["http", "https", "mailto"];

/// Browsers strip tab, line feed, and carriage return from anywhere inside
/// a URL before parsing it, so `java\tscript:` reaches the browser as a
/// live `javascript:` scheme no matter how it was analyzed. Normalizing
/// before analysis *and* emission means the safety check sees exactly what
/// the browser will act on, and the emitted HTML carries the same text.
fn removing_url_line_breaks(value: &str) -> String {
    let value = swift_text::replacing_occurrences(value, "\t", "");
    let value = swift_text::replacing_occurrences(&value, "\n", "");
    swift_text::replacing_occurrences(&value, "\r", "")
}

/// `CharacterSet.alphanumerics.union(CharacterSet(charactersIn: "+-."))`.
fn is_scheme_character(c: char) -> bool {
    CharSet::Alphanumerics.contains(c) || matches!(c, '+' | '-' | '.')
}

fn link_destination_is_safe(destination: &str) -> bool {
    let trimmed = swift_text::lowercased(swift_text::trimming(
        &removing_url_line_breaks(destination),
        CharSet::WhitespacesAndNewlines,
    ));
    let Some(colon) = swift_text::first_index_of(&trimmed, ':') else { return true };
    let prefix = &trimmed[..colon];
    if prefix.chars().any(|c| !is_scheme_character(c)) {
        // A character a URL scheme cannot contain means this parses as a
        // relative path (browsers never treat it as a scheme), so it is
        // inert by construction — no fuzzy substring block needed.
        return true;
    }
    LINK_SCHEME_ALLOWLIST.iter().any(|allowed| is(allowed, prefix))
}

fn image_source_is_safe(source: &str) -> bool {
    if swift_text::has_prefix(source, "/")
        || swift_text::has_prefix(source, "\\")
        || has_url_scheme(&removing_url_line_breaks(source))
    {
        return false;
    }
    let decoded = removing_percent_encoding(source).unwrap_or_else(|| source.to_owned());
    // `.split(separator: "#", maxSplits: 1)[0]` traps when the split is empty
    // (a source of nothing but `#`s, or a `?` alone after the fragment is cut).
    // Downright crashes there, and so does this port.
    let first = swift_text::split(&decoded, '#', 1, true).first().copied().unwrap_or_else(|| swift_trap());
    let first = swift_text::split(first, '?', 1, true).first().copied().unwrap_or_else(|| swift_trap());
    let path = swift_text::replacing_occurrences(first, "\\", "/");
    !swift_text::split(&path, '/', usize::MAX, false).iter().any(|component| is(component, ".."))
}

fn has_url_scheme(source: &str) -> bool {
    let Some(colon) = swift_text::first_index_of(source, ':') else { return false };
    let prefix = &source[..colon];
    if prefix.is_empty() {
        return false;
    }
    prefix.chars().all(is_scheme_character)
}

/// A Swift runtime trap (a failed `_precondition` in an optimized build): no
/// message; the process dies of `SIGTRAP` on arm64 (`brk #1`) and of
/// `SIGILL` on x86_64 (`ud2`), as Swift's does.
pub fn swift_trap() -> ! {
    #[cfg(target_arch = "aarch64")]
    unsafe {
        std::arch::asm!("brk #1", options(noreturn));
    }
    #[cfg(target_arch = "x86_64")]
    unsafe {
        std::arch::asm!("ud2", options(noreturn));
    }
    #[allow(unreachable_code)]
    {
        std::process::abort()
    }
}

// MARK: - Foundation string and regular-expression calls

/// `s as NSString`, built from UTF-16 so a leading U+FEFF survives.
fn ns(s: &str) -> Retained<NSString> {
    if s.is_ascii() {
        return NSString::from_str(s);
    }
    let units: Vec<u16> = s.encode_utf16().collect();
    swift_text::ns::foundation::ns_from_utf16(&units)
}

fn string(s: &NSString) -> String {
    swift_text::ns::foundation::to_string(s)
}

/// `value.replacingOccurrences(of: pattern, with: template, options: .regularExpression)`.
fn regex_replace(value: &str, pattern: &str, template: &str) -> String {
    autoreleasepool(|_| {
        let source = ns(value);
        let result = source.stringByReplacingOccurrencesOfString_withString_options_range(
            &NSString::from_str(pattern),
            &NSString::from_str(template),
            NSStringCompareOptions::RegularExpressionSearch,
            FRange::new(0, source.length()),
        );
        string(&result)
    })
}

/// `try? NSRegularExpression(pattern:)`.
fn regular_expression(pattern: &str) -> Option<Retained<NSRegularExpression>> {
    NSRegularExpression::regularExpressionWithPattern_options_error(
        &NSString::from_str(pattern),
        NSRegularExpressionOptions::empty(),
    )
    .ok()
}

/// `MarkdownCLI.replacingMatches(in:pattern:transform:)`: matches are
/// replaced from the last to the first, at the UTF-16 ranges
/// `NSRegularExpression` reports (Swift's `Range(_:in:)` accepts ranges that
/// are not Character-aligned, so every match is replaced).
fn replacing_matches(value: &str, pattern: &str, transform: impl Fn(&[String]) -> String) -> String {
    autoreleasepool(|_| {
        let Some(expression) = regular_expression(pattern) else { return value.to_owned() };
        let source = ns(value);
        let matches =
            expression.matchesInString_options_range(&source, NSMatchingOptions::empty(), FRange::new(0, source.length()));
        let mut result: Vec<u16> = value.encode_utf16().collect();
        for index in (0..matches.count()).rev() {
            let found = matches.objectAtIndex(index);
            let captures: Vec<String> = (1..found.numberOfRanges())
                .map(|capture| {
                    let range = found.rangeAtIndex(capture);
                    if range.location == objc2_foundation::NSNotFound as usize {
                        String::new()
                    } else {
                        string(&source.substringWithRange(range))
                    }
                })
                .collect();
            let range = found.range();
            if range.location + range.length > result.len() {
                continue;
            }
            let replacement: Vec<u16> = transform(&captures).encode_utf16().collect();
            result.splice(range.location..range.location + range.length, replacement);
        }
        String::from_utf16_lossy(&result)
    })
}

/// `line.range(of: pattern, options: .regularExpression)?.upperBound`, as a
/// byte offset into `line`.
fn regex_prefix_end(line: &str, pattern: &str) -> Option<usize> {
    autoreleasepool(|_| {
        let source = ns(line);
        let range =
            source.rangeOfString_options(&NSString::from_str(pattern), NSStringCompareOptions::RegularExpressionSearch);
        if range.location == objc2_foundation::NSNotFound as usize {
            return None;
        }
        Some(utf16_offset_to_byte(line, range.location + range.length))
    })
}

fn utf16_offset_to_byte(s: &str, offset: usize) -> usize {
    let mut units = 0;
    for (index, c) in s.char_indices() {
        if units >= offset {
            return index;
        }
        units += c.len_utf16();
    }
    s.len()
}

/// `value.removingPercentEncoding`.
fn removing_percent_encoding(value: &str) -> Option<String> {
    autoreleasepool(|_| ns(value).stringByRemovingPercentEncoding().map(|decoded| string(&decoded)))
}

fn heading(line: &str) -> Option<(usize, &str)> {
    let count = swift_text::graphemes(line).take_while(|g| swift_text::char_is(g, '#')).count();
    if !(1..=6).contains(&count) {
        return None;
    }
    let rest = swift_text::drop_first(line, count);
    if !swift_text::first(rest).is_some_and(|g| swift_text::char_is(g, ' ')) {
        return None;
    }
    Some((count, swift_text::drop_first(line, count + 1)))
}

struct ListItem {
    ordered: bool,
    checkbox: Option<bool>,
    text: String,
}

fn list_item(line: &str) -> Option<ListItem> {
    if let Some(end) = regex_prefix_end(line, r"^\s*(\d+)[.)]\s+") {
        return Some(ListItem { ordered: true, checkbox: None, text: line[end..].to_owned() });
    }
    let end = regex_prefix_end(line, r"^\s*[-*+]\s+")?;
    let text = &line[end..];
    if swift_text::has_prefix(text, "[x] ") || swift_text::has_prefix(text, "[X] ") {
        return Some(ListItem { ordered: false, checkbox: Some(true), text: swift_text::drop_first(text, 4).to_owned() });
    }
    if swift_text::has_prefix(text, "[ ] ") {
        return Some(ListItem { ordered: false, checkbox: Some(false), text: swift_text::drop_first(text, 4).to_owned() });
    }
    Some(ListItem { ordered: false, checkbox: None, text: text.to_owned() })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Swift 6.4 `Double(_:)`, recorded by a probe on macOS 26.
    #[test]
    fn swift_double_matches_the_swift_parser() {
        let accepted: &[(&str, f64)] = &[
            ("750", 750.0),
            ("1e3", 1000.0),
            ("0x10", 16.0),
            ("0x1p3", 8.0),
            ("+5", 5.0),
            ("-0", -0.0),
            ("1e400", f64::INFINITY),
            ("1e-400", 0.0),
            (".5", 0.5),
            ("5.", 5.0),
            ("0x.8", 0.5),
            ("0x1.8p1", 3.0),
            ("1.e5", 100000.0),
            ("0X10", 16.0),
            ("0x1p-99999", 0.0),
            ("0x1p99999", f64::INFINITY),
            ("0x1.", 1.0),
            ("0x1p+3", 8.0),
            ("INF", f64::INFINITY),
            ("Infinity", f64::INFINITY),
            ("+inf", f64::INFINITY),
            ("00.5", 0.5),
            ("1e-0", 1.0),
            ("1e0400", f64::INFINITY),
            ("-.5", -0.5),
        ];
        for (text, value) in accepted {
            let parsed = swift_double(text).unwrap_or_else(|| panic!("{text} should parse"));
            assert!(parsed == *value && parsed.is_sign_negative() == value.is_sign_negative(), "{text}: {parsed}");
        }
        for text in [
            " 5", "5 ", "", "1_000", "٣", "5\n", "e5", "-", "+", "0x", "1e", "1e+", "infinit", ".e5", ".", "0x.", "0xp1",
            "1ee3", "infinityx", "nanx", "1e3.5", "0b101", "1,5", "\u{0}5",
        ] {
            assert!(swift_double(text).is_none(), "{text:?} should not parse");
        }
        assert!(swift_double("nan").unwrap().is_nan());
        assert!(swift_double("NaN").unwrap().is_nan());
        assert!(swift_double("nan(123)").unwrap().is_nan());
    }
}
