//! Port of `Tests/MarkdownCLITests/MarkdownCLITests.swift`.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use upleft_cli::doctor::DownDoctor;
use upleft_cli::markdown_cli::{self, Action, ExportFormat, OpenOptions, ParseError};
use upleft_core::compatibility::render_target::{BuiltInRenderTarget, MarkdownCapability};
use upleft_foundation::foundation_io;
use upleft_foundation::url::FileUrl;

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

fn parse(values: &[&str]) -> Result<Action, ParseError> {
    markdown_cli::parse(&args(values))
}

fn html(markdown: &str) -> String {
    markdown_cli::html(markdown, "Markdown")
}

/// `FileManager.default.temporaryDirectory.appendingPathComponent("<prefix>-\(UUID().uuidString)")`.
fn temporary_directory(prefix: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("{prefix}-{}", foundation_io::uuid_string()));
    std::fs::create_dir_all(&path).unwrap();
    path
}

struct Removing(PathBuf);

impl Drop for Removing {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn default_action_is_open() {
    let action = parse(&["README.md"]).unwrap();
    assert_eq!(action, Action::Open(OpenOptions::default(), args(&["README.md"])));
}

#[test]
fn commands_parse_their_options() {
    assert_eq!(parse(&["read", "--json", "-"]).unwrap(), Action::Read { json: true, paths: args(&["-"]) });
    assert_eq!(
        parse(&["export", "-f", "html", "-o", "out.html", "doc.md"]).unwrap(),
        Action::Export { format: ExportFormat::Html, output: Some("out.html".into()), paths: args(&["doc.md"]) }
    );
    assert_eq!(
        parse(&["check", "--json", "--target", "github", "doc.md"]).unwrap(),
        Action::Check { json: true, target: Some(BuiltInRenderTarget::GitHub), paths: args(&["doc.md"]) }
    );
    assert_eq!(parse(&["outline", "--json", "doc.md"]).unwrap(), Action::Outline { json: true, paths: args(&["doc.md"]) });

    let open_options = OpenOptions { line: Some(42), review: true, edit: true, ..OpenOptions::default() };
    assert_eq!(parse(&["open", "--line", "42", "--review", "doc.md"]).unwrap(), Action::Open(open_options, args(&["doc.md"])));
    let reveal_options = OpenOptions { reveal: true, ..OpenOptions::default() };
    assert_eq!(parse(&["open", "--reveal", "doc.md"]).unwrap(), Action::Open(reveal_options, args(&["doc.md"])));
    assert_eq!(parse(&["doctor", "--json"]).unwrap(), Action::Doctor { json: true, app_path: None });
    assert_eq!(
        parse(&["doctor", "--app", "/tmp/Downright.app"]).unwrap(),
        Action::Doctor { json: false, app_path: Some("/tmp/Downright.app".into()) }
    );
}

#[test]
fn bad_arguments_have_actionable_errors() {
    assert_eq!(parse(&["read", "--wat"]), Err(ParseError::UnknownOption("--wat".into())));
    assert_eq!(parse(&["export", "-o"]), Err(ParseError::MissingValue("-o".into())));
    assert_eq!(parse(&["open", "--line"]), Err(ParseError::MissingValue("--line".into())));
    assert_eq!(parse(&["open", "--line", "0"]), Err(ParseError::InvalidLine("0".into())));
    assert_eq!(parse(&["doctor", "--app"]), Err(ParseError::MissingValue("--app".into())));
}

#[test]
fn html_export_is_self_contained_and_escaped() {
    let html = html("# Hello <world>\n\n- [x] done\n\n`code`");
    assert!(html.contains("<h1>Hello &lt;world&gt;</h1>"));
    assert!(html.contains("<input type=\"checkbox\" disabled checked>"));
    assert!(html.contains("<code>code</code>"));
    assert!(!html.contains("http://") && !html.contains("https://"));
}

/// Regression: the writer joined paragraphs, code lines, and body blocks
/// with the two characters `\` + `n` instead of a newline, so every
/// multi-line paragraph and every code block exported as one line of text
/// with visible `\n` garbage in it.
#[test]
fn html_export_uses_real_newlines() {
    let html = html("line one\nline two\n\n```swift\nlet a = 1\nlet b = 2\n```\n");
    assert!(html.contains("<p>line one<br>\nline two</p>"));
    assert!(html.contains(">let a = 1\nlet b = 2</code>"));
    assert!(!html.contains("\\n"), "no literal backslash-n may appear in the output");
}

#[test]
fn html_export_makes_unsafe_links_inert() {
    let html = html(
        "[relative](guide.md) [anchor](#part) [web](https://example.com) [mail](mailto:a@example.com)\n\
         [script](JaVaScRiPt:alert(1)) [data](data:text/html,bad) [editor](vscode://file/tmp/a)",
    );

    assert!(html.contains("href=\"guide.md\""));
    assert!(html.contains("href=\"#part\""));
    assert!(html.contains("href=\"https://example.com\""));
    assert!(html.contains("href=\"mailto:a@example.com\""));
    assert!(!html.to_lowercase().contains("href=\"javascript:"));
    assert!(!html.to_lowercase().contains("href=\"data:"));
    assert!(!html.to_lowercase().contains("href=\"vscode:"));
}

#[test]
fn html_export_rejects_obfuscated_schemes() {
    let html = html("[bad](javascript/foo:alert(1))");
    assert!(!html.contains("href=\"javascript/foo:alert(1)\""));
}

#[test]
fn html_export_neutralizes_line_break_obfuscated_schemes() {
    // Browsers strip tab, LF, and CR from anywhere inside a URL before
    // parsing it, so every destination below would reach the browser as a
    // live scheme unless the exporter normalizes before analyzing.
    let html = html(
        "[tab](java\tscript:alert(1)) [newline](ja\nscript:alert(1)) [cr](j\rascript:alert(2))\n\
         ![pixel](http\t://tracking.example/x.png)",
    );

    assert!(!html.to_lowercase().contains("href=\"javascript:"));
    assert!(!html.contains('\t'), "no raw tab may survive into an emitted URL");
    assert!(!html.contains('\r'), "no raw carriage return may survive into an emitted URL");
    assert!(!html.contains("<img "), "an obfuscated remote image source must not become a live img");

    let safe = self::html("[web](https://example.com) [rel](./a.md)");
    assert!(safe.contains("href=\"https://example.com\""));
    assert!(safe.contains("href=\"./a.md\""));
}

#[test]
fn check_and_outline_support_double_dash() {
    assert_eq!(parse(&["check", "--", "-notes.md"]).unwrap(), Action::Check { json: false, target: None, paths: args(&["-notes.md"]) });
    assert_eq!(parse(&["outline", "--", "-draft.md"]).unwrap(), Action::Outline { json: false, paths: args(&["-draft.md"]) });
}

#[test]
fn outline_calculates_lines_on_cr_and_crlf() {
    let cr = "Line 1\r# Heading 1\rLine 3\r## Heading 2\r";
    let outline_cr = markdown_cli::outline(cr);
    assert_eq!(outline_cr.len(), 2);
    assert_eq!(outline_cr[0].line, 2);
    assert_eq!(outline_cr[1].line, 4);

    let crlf = "Line 1\r\n# Heading 1\r\nLine 3\r\n## Heading 2\r\n";
    let outline_crlf = markdown_cli::outline(crlf);
    assert_eq!(outline_crlf.len(), 2);
    assert_eq!(outline_crlf[0].line, 2);
    assert_eq!(outline_crlf[1].line, 4);
}

#[test]
fn html_export_never_retains_external_image_sources() {
    let html = html(
        "![relative](images/photo.png)\n\
         ![web](https://tracker.example/pixel.png)\n\
         ![data](data:image/svg+xml,bad)\n\
         ![file](file:///private/etc/passwd)\n\
         ![protocol](//tracker.example/pixel.png)\n\
         ![absolute](/private/etc/passwd)\n\
         ![traversal](../private/photo.png)\n\
         ![encoded](%2e%2e/private/photo.png)",
    );

    assert!(html.contains("src=\"images/photo.png\""));
    assert!(!html.contains("src=\"https://"));
    assert!(!html.contains("src=\"data:"));
    assert!(!html.contains("src=\"file:"));
    assert!(!html.contains("src=\"//"));
    assert!(!html.contains("src=\"/private"));
    assert!(!html.contains("src=\"../"));
    assert!(!html.contains("src=\"%2e%2e"));
    assert_eq!(html.split("class=\"missing-image\"").count() - 1, 7);
}

#[test]
fn settings_loader_distinguishes_absent_from_invalid_files() {
    let root = temporary_directory("down-settings");
    let _cleanup = Removing(root.clone());

    let missing = root.join("missing.json");
    let loaded = markdown_cli::load_settings_default(&FileUrl::from_path(missing.to_str().unwrap())).unwrap();
    assert!(loaded.as_object().unwrap().is_empty());

    let malformed = root.join("malformed.json");
    let malformed_bytes = b"{ not json".to_vec();
    std::fs::write(&malformed, &malformed_bytes).unwrap();
    assert!(markdown_cli::load_settings_default(&FileUrl::from_path(malformed.to_str().unwrap())).is_err());
    assert_eq!(std::fs::read(&malformed).unwrap(), malformed_bytes);

    let array = root.join("array.json");
    let array_bytes = b"[]".to_vec();
    std::fs::write(&array, &array_bytes).unwrap();
    assert!(markdown_cli::load_settings_default(&FileUrl::from_path(array.to_str().unwrap())).is_err());
    assert_eq!(std::fs::read(&array).unwrap(), array_bytes);
}

#[test]
fn settings_loader_caps_reads_without_changing_the_file() {
    let root = temporary_directory("down-settings-large");
    let _cleanup = Removing(root.clone());
    let url = root.join("settings.json");
    let bytes = vec![0x20u8; 65];
    std::fs::write(&url, &bytes).unwrap();

    assert!(markdown_cli::load_settings(&FileUrl::from_path(url.to_str().unwrap()), 64).is_err());
    assert_eq!(std::fs::read(&url).unwrap(), bytes);
}

#[test]
fn settings_loader_rejects_unreadable_file_kinds() {
    let root = temporary_directory("down-settings-kind");
    let _cleanup = Removing(root.clone());

    assert!(markdown_cli::load_settings_default(&FileUrl::from_path(root.to_str().unwrap())).is_err());
}

#[test]
fn hook_install_fails_without_changing_malformed_settings() {
    assert_hook_install_refuses(b"{ user-owned and damaged".to_vec());
}

#[test]
fn hook_install_fails_without_changing_oversized_settings() {
    assert_hook_install_refuses(vec![0x20u8; markdown_cli::MAXIMUM_SETTINGS_BYTES + 1]);
}

#[test]
fn hook_install_fails_without_changing_unreadable_settings() {
    use std::os::unix::fs::PermissionsExt;
    let root = hook_test_root();
    let _cleanup = Removing(root.clone());
    let settings = root.join(".claude/settings.json");
    let original = br#"{"permissions":{"allow":[]}}"#.to_vec();
    std::fs::write(&settings, &original).unwrap();
    std::fs::set_permissions(&settings, std::fs::Permissions::from_mode(0)).unwrap();

    let status = run_hook_install(&root);
    assert_ne!(status, 0);
    std::fs::set_permissions(&settings, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(std::fs::read(&settings).unwrap(), original);
}

#[test]
fn health_findings_are_deterministic() {
    let markdown = "[broken](missing.md)\n";
    let first = markdown_cli::diagnostics(markdown, None);
    let second = markdown_cli::diagnostics(markdown, None);
    assert_eq!(
        first.iter().map(|diagnostic| diagnostic.id.clone()).collect::<Vec<_>>(),
        second.iter().map(|diagnostic| diagnostic.id.clone()).collect::<Vec<_>>()
    );
}

#[test]
fn outline_and_target_checks_use_core_parser() {
    let markdown = "# First\n\n## Child\n\n[^1]: note\n";
    assert_eq!(
        markdown_cli::outline(markdown).iter().map(|item| item.title.clone()).collect::<Vec<_>>(),
        vec!["First", "Child"]
    );
    assert!(
        markdown_cli::compatibility_diagnostics(markdown, BuiltInRenderTarget::CommonMark)
            .iter()
            .any(|diagnostic| diagnostic.capability == MarkdownCapability::Footnotes)
    );
    assert_eq!(markdown_cli::render_target("CommonMark"), Some(BuiltInRenderTarget::CommonMark));
}

#[test]
fn doctor_plugin_parsing_only_accepts_our_enabled_registration() {
    assert!(DownDoctor::plugin_is_enabled(
        "+ com.ezzy.downright.quicklook(1.0)\n- com.other.quicklook(1.0)",
        "com.ezzy.downright.quicklook"
    ));
    assert!(!DownDoctor::plugin_is_enabled("- com.ezzy.downright.quicklook(1.0)", "com.ezzy.downright.quicklook"));
    assert!(!DownDoctor::plugin_is_enabled("+ com.other.quicklook(1.0)", "com.ezzy.downright.quicklook"));
}

#[test]
fn folder_check_skips_build_trees() {
    let root = std::env::temp_dir().join(format!("down-check-folder-{}", foundation_io::uuid_string()));
    let _cleanup = Removing(root.clone());
    std::fs::create_dir_all(root.join(".build")).unwrap();
    std::fs::write(root.join("keep.md"), "# Keep\n\n[missing](does-not-exist.md)\n").unwrap();
    std::fs::write(root.join(".build/ignore.md"), "# Ignore\n\n[missing](also-missing.md)\n").unwrap();

    let (status, stdout) = run_down(&["check", "--json", root.to_str().unwrap()], &root);

    assert_eq!(status, 1);
    assert!(stdout.contains("keep.md"));
    assert!(!stdout.contains("ignore.md"));
}

fn assert_hook_install_refuses(original: Vec<u8>) {
    let root = hook_test_root();
    let _cleanup = Removing(root.clone());
    let settings = root.join(".claude/settings.json");
    std::fs::write(&settings, &original).unwrap();

    assert_ne!(run_hook_install(&root), 0);
    assert_eq!(std::fs::read(&settings).unwrap(), original);
}

fn hook_test_root() -> PathBuf {
    let root = std::env::temp_dir().join(format!("down-hook-install-{}", foundation_io::uuid_string()));
    std::fs::create_dir_all(root.join(".claude")).unwrap();
    root
}

fn run_hook_install(directory: &Path) -> i32 {
    run_down(&["hook", "--install", "--scope", "project"], directory).0
}

/// Runs the built `down` in `directory` with stdin from /dev/null. HOME and
/// CFFIXED_USER_HOME point at the directory so nothing can reach the real
/// home (the Swift test inherits them; its commands only touch the project).
fn run_down(arguments: &[&str], directory: &Path) -> (i32, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_down"))
        .args(arguments)
        .current_dir(directory)
        .env("HOME", directory)
        .env("CFFIXED_USER_HOME", directory)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .unwrap();
    (output.status.code().unwrap_or(-1), String::from_utf8_lossy(&output.stdout).into_owned())
}
