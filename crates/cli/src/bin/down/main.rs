//! `down`: port of `Sources/down/main.swift`, Downright's terminal launcher.
//!
//! Same exit codes, same bytes on stdout and stderr, same `open` and
//! `mdfind` invocations. `print` output goes through a buffer flushed at
//! exit, as Swift's (C stdio) does; `FileHandle.standardOutput.write` and
//! `FileHandle.standardError.write` are unbuffered writes.

use std::io::{Read, Write};
use std::os::unix::process::ExitStatusExt;
use std::process::{Command, Stdio};
use std::sync::Mutex;

use upleft_cli::agent_bridge;
use upleft_cli::agent_watcher::AgentWatcher;
use upleft_cli::doctor::DownDoctor;
use upleft_cli::markdown_cli::{self, Action, HookMode, OpenOptions};
use upleft_foundation::foundation_io;
use upleft_foundation::json_encoder::{self, OutputFormatting};
use upleft_foundation::json_serialization::{self, AnyJson, WritingOptions};
use upleft_foundation::url::{self, FileUrl};
use upleft_swift_text as swift_text;

// MARK: - Output

/// Swift's `print` buffer (C `stdout`, fully buffered on a pipe).
static PRINTED: Mutex<Vec<u8>> = Mutex::new(Vec::new());

/// `print(_:terminator:)`.
fn print_with(text: &str, terminator: &str) {
    let mut buffer = PRINTED.lock().unwrap();
    buffer.extend_from_slice(text.as_bytes());
    buffer.extend_from_slice(terminator.as_bytes());
    if buffer.len() >= 1 << 16 {
        let _ = write_fd(1, &buffer);
        buffer.clear();
    }
}

fn print(text: &str) {
    print_with(text, "\n");
}

fn flush_printed() {
    let mut buffer = PRINTED.lock().unwrap();
    if !buffer.is_empty() {
        let _ = write_fd(1, &buffer);
        buffer.clear();
    }
}

fn write_fd(fd: i32, mut bytes: &[u8]) -> std::io::Result<()> {
    while !bytes.is_empty() {
        let written = unsafe { libc::write(fd, bytes.as_ptr().cast(), bytes.len()) };
        if written < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        bytes = &bytes[written as usize..];
    }
    Ok(())
}

/// `FileHandle.standardOutput.write(_:)`.
fn write_stdout(bytes: &[u8]) {
    let _ = write_fd(1, bytes);
}

/// `FileHandle.standardError.write(_:)`.
fn write_stderr(bytes: &[u8]) {
    let _ = write_fd(2, bytes);
}

/// `exit(_:)`: flushes stdio, then exits.
fn exit(status: i32) -> ! {
    flush_printed();
    let _ = std::io::stdout().flush();
    std::process::exit(status)
}

fn write_error(message: &str, status: i32) -> ! {
    write_stderr(format!("down: {message}\n").as_bytes());
    exit(status)
}

/// `FileHandle.standardInput.readDataToEndOfFile()`.
fn read_standard_input() -> Vec<u8> {
    let mut data = Vec::new();
    let mut input = unsafe { <std::fs::File as std::os::fd::FromRawFd>::from_raw_fd(0) };
    let _ = input.read_to_end(&mut data);
    std::mem::forget(input);
    data
}

fn stdin_is_a_tty() -> bool {
    unsafe { libc::isatty(0) != 0 }
}

// MARK: - Inputs

fn read_inputs(paths: &[String], maximum_bytes: Option<usize>) -> Vec<(String, String)> {
    let requested: Vec<String> = if paths.is_empty() { vec!["-".into()] } else { paths.to_vec() };
    requested
        .iter()
        .map(|path| {
            if path == "-" {
                let data = read_standard_input();
                if data.is_empty() {
                    write_error("stdin is empty", 66);
                }
                if let Some(maximum_bytes) = maximum_bytes
                    && data.len() > maximum_bytes
                {
                    write_error(&format!("stdin exceeds the {} MB check limit", maximum_bytes / 1_024 / 1_024), 65);
                }
                return ("stdin".to_owned(), String::from_utf8_lossy(&data).into_owned());
            }
            let url = FileUrl::from_path(&url::expanding_tilde_in_path(path)).standardized_file_url();
            if !foundation_io::file_exists(&url.path()) {
                write_error(&format!("{path}: no such file"), 66);
            }
            if let Some(maximum_bytes) = maximum_bytes
                && let Some(size) = resource_file_size(&url.path())
                && size > maximum_bytes as u64
            {
                write_error(&format!("{path}: file exceeds the {} MB check limit", maximum_bytes / 1_024 / 1_024), 65);
            }
            match foundation_io::string_contents_of_utf8(&url) {
                Ok(text) => (url.path(), text),
                Err(error) => write_error(&format!("{path}: cannot read UTF-8 Markdown ({})", error.description), 65),
            }
        })
        .collect()
}

/// `url.resourceValues(forKeys: [.fileSizeKey]).fileSize`: the item itself
/// (a symbolic link reports its own size), nothing for a directory.
fn resource_file_size(path: &str) -> Option<u64> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    (metadata.is_file() || metadata.file_type().is_symlink()).then_some(metadata.len())
}

fn expanded_markdown_paths(paths: &[String]) -> Vec<String> {
    let ignored_directories = [".git", ".build", "node_modules", "DerivedData", ".swiftpm"];
    let maximum_files = 1_000;
    let mut results: Vec<String> = Vec::new();
    for path in paths {
        if path == "-" {
            results.push(path.clone());
            continue;
        }
        let expanded = url::expanding_tilde_in_path(path);
        let (exists, is_directory) = foundation_io::file_exists_is_directory(&expanded);
        if !(exists && is_directory) {
            results.push(path.clone());
            continue;
        }
        let base = FileUrl::from_path(&expanded);
        let _ = foundation_io::enumerate_directory(&expanded, |item| {
            let name = last_path_component(item);
            if ignored_directories.iter().any(|ignored| swift_text::str_eq(ignored, &name)) {
                return true;
            }
            let full = base.appending_path_component(item).path();
            if !markdown_cli::is_markdown_path(&full) {
                return false;
            }
            results.push(full);
            if results.len() > maximum_files {
                write_error(&format!("folder check exceeds the {maximum_files}-file limit"), 65);
            }
            false
        });
    }
    results.sort_by(|a, b| swift_text::str_cmp(a, b));
    results
}

/// `(item as NSString).lastPathComponent`.
fn last_path_component(path: &str) -> String {
    objc2_foundation::NSString::from_str(path).lastPathComponent().to_string()
}

/// 1-based source line containing the UTF-16 `offset`, with the same
/// line-ending semantics as MarkdownCore's line index (LF, CRLF, lone CR).
/// Diagnostics print in the conventional `file:line:` shape, so an offset
/// must never be shown in the line-number position.
fn line_number(offset: isize, text: &str) -> isize {
    let units: Vec<u16> = text.encode_utf16().collect();
    let clamped = offset.min(units.len() as isize).max(0) as usize;
    let mut line = 1;
    let mut i = 0;
    while i < clamped {
        let c = units[i];
        if c == 0x0A {
            line += 1;
            i += 1;
        } else if c == 0x0D {
            line += 1;
            i += if i + 1 < clamped && units[i + 1] == 0x0A { 2 } else { 1 };
        } else {
            i += 1;
        }
    }
    line
}

fn stdin_file() -> Option<FileUrl> {
    if stdin_is_a_tty() {
        return None;
    }
    let data = read_standard_input();
    if data.is_empty() {
        return None;
    }
    let directory = foundation_io::temporary_directory().appending_path_component_is_directory("Downright", true);
    let _ = foundation_io::create_directory(&directory, true);
    let url = directory.appending_path_component(&format!("stdin-{}.md", foundation_io::uuid_string()));
    match foundation_io::write_atomically(&data, &url) {
        Ok(()) => Some(url),
        Err(error) => write_error(&format!("cannot create stdin document: {}", error.description), 70),
    }
}

fn locate_app() -> Option<FileUrl> {
    let candidates = [
        "/Applications/Downright.app".to_owned(),
        format!("{}/Applications/Downright.app", foundation_io::ns_home_directory()),
        url::current_directory_path() + "/.build/bundle/Downright.app",
    ];
    if let Some(path) = candidates.iter().find(|path| foundation_io::file_exists(path)) {
        return Some(FileUrl::from_path(path));
    }
    if !std::path::Path::new("/usr/bin/mdfind").exists() {
        return None;
    }
    let output = Command::new("/usr/bin/mdfind")
        .arg("kMDItemCFBundleIdentifier == 'com.ezzy.downright'")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    swift_text::split_default(&text, '\n').first().map(|first| FileUrl::from_path(first))
}

/// Launches Downright for a set of files, returning the `open` exit status.
///
/// Shared by `open`, `notify`, and `watch` so the three agree on how the app
/// is located and what "background" means.
fn launch(paths: &[String], options: &OpenOptions) -> i32 {
    if paths.is_empty() {
        return 69;
    }
    let Some(app) = locate_app() else { return 69 };
    let mut open_arguments = vec!["-a".to_owned(), app.path()];
    if options.new_window {
        open_arguments.push("-n".into());
    }
    if options.background {
        open_arguments.push("-g".into());
    }
    if options.wait {
        open_arguments.push("-W".into());
    }
    open_arguments.push("--".into());
    open_arguments.extend(paths.iter().cloned());
    let mut app_arguments: Vec<String> = Vec::new();
    if options.edit {
        app_arguments.extend(["--mode".into(), "live".into()]);
    }
    if let Some(line) = options.line {
        app_arguments.extend(["--downright-line".into(), line.to_string()]);
    }
    if options.review {
        app_arguments.push("--downright-review".into());
    }
    if !app_arguments.is_empty() {
        open_arguments.push("--args".into());
        open_arguments.extend(app_arguments);
    }
    match Command::new("/usr/bin/open").args(&open_arguments).status() {
        Ok(status) => status.code().or(status.signal()).unwrap_or(0),
        Err(_) => 70,
    }
}

/// Reveals files in Finder without requiring Downright to be installed.
fn reveal(paths: &[String]) -> i32 {
    if paths.is_empty() {
        return 64;
    }
    if paths.iter().any(|path| path == "-") {
        write_error("--reveal requires file paths, not stdin", 64);
    }
    let urls: Vec<FileUrl> = paths
        .iter()
        .map(|path| FileUrl::from_path(&url::expanding_tilde_in_path(path)).standardized_file_url())
        .collect();
    for url in &urls {
        if !foundation_io::file_exists(&url.path()) {
            write_error(&format!("{}: no such file", url.path()), 66);
        }
    }
    objc2::rc::autoreleasepool(|_| {
        let urls: Vec<_> = urls.iter().map(FileUrl::to_nsurl).collect();
        let array = objc2_foundation::NSArray::from_retained_slice(&urls);
        objc2_app_kit::NSWorkspace::sharedWorkspace().activateFileViewerSelectingURLs(&array);
    });
    0
}

/// The command an installed hook should run.
///
/// A hook does not inherit an interactive shell's `PATH`, so `down` alone can
/// resolve in a terminal and then fail silently inside the agent. Prefer the
/// stable install locations, and fall back to this process's own absolute
/// path.
fn resolved_executable() -> String {
    let installed = ["/usr/local/bin/down", "/opt/homebrew/bin/down"];
    if let Some(path) = installed.iter().find(|path| foundation_io::file_exists(path)) {
        return (*path).to_owned();
    }
    let argv0 = std::env::args_os().next().map(|argument| argument.to_string_lossy().into_owned()).unwrap_or("down".into());
    if argv0.starts_with('/') {
        return argv0;
    }
    let resolved = FileUrl::from_path_relative_to(&argv0, &FileUrl::from_path(&url::current_directory_path()));
    if foundation_io::file_exists(&resolved.path()) { resolved.standardized_file_url().path() } else { "down".into() }
}

fn main() {
    // Swift leaves SIGPIPE at its default action; Rust ignores it at startup.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    let arguments: Vec<String> =
        std::env::args_os().skip(1).map(|argument| argument.to_string_lossy().into_owned()).collect();

    let action = match markdown_cli::parse(&arguments) {
        Ok(action) => action,
        Err(error) => write_error(&error.to_string(), 64),
    };

    match action {
        Action::Help => print(&markdown_cli::usage()),
        Action::Version => print(&format!("down {}", markdown_cli::VERSION)),
        Action::Read { json, paths } => {
            let inputs = read_inputs(&paths, None);
            if json {
                let values = AnyJson::Array(
                    inputs
                        .iter()
                        .map(|(path, markdown)| {
                            AnyJson::Object(vec![
                                ("path".into(), AnyJson::String(path.clone())),
                                ("markdown".into(), AnyJson::String(markdown.clone())),
                            ])
                        })
                        .collect(),
                );
                let Ok(data) = json_serialization::data(
                    &values,
                    WritingOptions { pretty_printed: true, sorted_keys: true, ..Default::default() },
                ) else {
                    write_error("cannot encode JSON", 70)
                };
                write_stdout(&data);
                write_stdout(b"\n");
            } else {
                for (index, (_, text)) in inputs.iter().enumerate() {
                    if index > 0 {
                        print_with("\n", "");
                    }
                    print_with(text, if swift_text::has_suffix(text, "\n") { "" } else { "\n" });
                }
            }
        }
        Action::Export { output, paths, .. } => {
            let inputs = read_inputs(&paths, None);
            let html = if inputs.len() == 1 {
                let title = FileUrl::from_path(&inputs[0].0).deleting_path_extension().last_path_component();
                markdown_cli::html(&inputs[0].1, &title)
            } else {
                let joined: Vec<&str> = inputs.iter().map(|(_, text)| text.as_str()).collect();
                markdown_cli::html(&joined.join("\n\n"), "Markdown export")
            };
            match output {
                Some(output) if output != "-" => {
                    if let Err(error) = foundation_io::write_atomically(html.as_bytes(), &FileUrl::from_path(&output)) {
                        write_error(&format!("cannot write {output}: {}", error.description), 73);
                    }
                }
                _ => write_stdout(html.as_bytes()),
            }
        }
        Action::Check { json, target, paths } => {
            let inputs = read_inputs(&expanded_markdown_paths(&paths), Some(10 * 1_024 * 1_024));
            let mut findings = 0;
            for (path, content) in &inputs {
                let base = if path == "stdin" { None } else { Some(FileUrl::from_path(path).deleting_last_path_component()) };
                let diagnostics = markdown_cli::diagnostics(content, base.as_ref());
                let compatibility = target
                    .map(|target| markdown_cli::compatibility_diagnostics(content, target))
                    .unwrap_or_default();
                findings += diagnostics.len() + compatibility.len();
                if json {
                    let mut values: Vec<AnyJson> = diagnostics
                        .iter()
                        .map(|diagnostic| {
                            AnyJson::Object(vec![
                                ("path".into(), AnyJson::String(path.clone())),
                                ("id".into(), AnyJson::String(diagnostic.id.clone())),
                                ("severity".into(), AnyJson::String(diagnostic.severity.raw_value().into())),
                                ("category".into(), AnyJson::String(diagnostic.category.raw_value().into())),
                                ("message".into(), AnyJson::String(diagnostic.message.clone())),
                                ("location".into(), AnyJson::Int(diagnostic.range.location as i64)),
                            ])
                        })
                        .collect();
                    values.extend(compatibility.iter().map(|diagnostic| {
                        AnyJson::Object(vec![
                            ("path".into(), AnyJson::String(path.clone())),
                            ("id".into(), AnyJson::String(diagnostic.id.clone())),
                            ("severity".into(), AnyJson::String(diagnostic.severity.raw_value().into())),
                            ("category".into(), AnyJson::String("compatibility".into())),
                            ("target".into(), AnyJson::String(target.map(|t| t.raw_value()).unwrap_or("").into())),
                            ("message".into(), AnyJson::String(diagnostic.title.clone())),
                            ("location".into(), AnyJson::Int(diagnostic.range.location as i64)),
                        ])
                    }));
                    let Ok(data) = json_serialization::data(
                        &AnyJson::Array(values),
                        WritingOptions { sorted_keys: true, ..Default::default() },
                    ) else {
                        write_error("cannot encode JSON", 70)
                    };
                    write_stdout(&data);
                    write_stdout(b"\n");
                } else {
                    for diagnostic in &diagnostics {
                        print(&format!(
                            "{path}:{}: {}: {} [{}]",
                            line_number(diagnostic.range.location, content),
                            diagnostic.severity.raw_value(),
                            diagnostic.message,
                            diagnostic.id
                        ));
                    }
                    for diagnostic in &compatibility {
                        print(&format!(
                            "{path}:{}: warning: {} [target:{}]",
                            line_number(diagnostic.range.location, content),
                            diagnostic.title,
                            target.map(|t| t.raw_value()).unwrap_or("unknown")
                        ));
                    }
                }
            }
            if findings > 0 {
                exit(1);
            }
        }
        Action::Outline { json, paths } => {
            let inputs = read_inputs(&paths, None);
            for (path, text) in &inputs {
                let outline = markdown_cli::outline(text);
                if json {
                    let value = json_encoder::JsonValue::Array(outline.iter().map(|item| item.json_value()).collect());
                    write_stdout(&json_encoder::encode(&value, OutputFormatting::default()));
                    write_stdout(b"\n");
                } else {
                    for heading in &outline {
                        print(&format!(
                            "{path}:{}: {}{} #{}",
                            heading.line,
                            swift_text::repeating("  ", (heading.level - 1).max(0)),
                            heading.title,
                            heading.slug
                        ));
                    }
                }
            }
        }
        Action::Doctor { json, app_path } => {
            let report = DownDoctor::run(app_path.as_deref());
            if json {
                print(&DownDoctor::json(&report));
            } else {
                print(&DownDoctor::human_readable(&report));
            }
            exit(if report.has_failures() { 1 } else { 0 });
        }
        Action::Open(options, paths) => {
            if options.reveal {
                exit(reveal(&paths));
            }
            let mut paths = paths;
            if paths.iter().any(|path| path == "-") {
                let Some(piped) = stdin_file() else { write_error("stdin is not available", 66) };
                paths.retain(|path| path != "-");
                paths.push(piped.path());
            } else if paths.is_empty()
                && let Some(piped) = stdin_file()
            {
                paths.push(piped.path());
            }
            if paths.is_empty() {
                print(&markdown_cli::usage());
                exit(64);
            }
            for path in &paths {
                if !foundation_io::file_exists(&FileUrl::from_path(path).path()) {
                    write_error(&format!("{path}: no such file"), 66);
                }
            }
            if locate_app().is_none() {
                write_error("could not find Downright.app; install it in /Applications or run Scripts/bundle-app.sh", 69);
            }
            exit(launch(&paths, &options));
        }
        Action::Notify(options) => {
            // A hook runs inside the agent's turn. Every failure path here
            // exits 0: an unreadable payload, a missing app, or a path that is
            // not Markdown are all "nothing to review", and none of them
            // justify failing somebody's edit.
            let data = if !stdin_is_a_tty() { read_standard_input() } else { Vec::new() };
            let targets = agent_bridge::openable_targets(&agent_bridge::hook_payload_paths(&data));
            if targets.is_empty() {
                exit(0);
            }
            if options.dry_run {
                for target in &targets {
                    print(target);
                }
                exit(0);
            }
            let open_options = OpenOptions { background: !options.focus, ..OpenOptions::default() };
            launch(&targets, &open_options);
            exit(0);
        }
        Action::Watch(options, paths) => {
            let roots: Vec<FileUrl> =
                (if paths.is_empty() { vec![url::current_directory_path()] } else { paths })
                    .iter()
                    .map(|path| FileUrl::from_path(&url::expanding_tilde_in_path(path)).standardized_file_url())
                    .collect();
            for root in &roots {
                if !foundation_io::file_exists(&root.deleting_last_path_component().path()) {
                    write_error(&format!("{}: no such file or folder", root.path()), 66);
                }
            }
            if locate_app().is_none() {
                write_error("could not find Downright.app; install it in /Applications or run Scripts/bundle-app.sh", 69);
            }
            let open_options = OpenOptions { background: !options.focus, ..OpenOptions::default() };
            let watcher = AgentWatcher::new(roots.clone(), options.debounce, move |urls| {
                for url in &urls {
                    write_stderr(format!("down: opening {}\n", url.path()).as_bytes());
                }
                let paths: Vec<String> = urls.iter().map(FileUrl::path).collect();
                launch(&paths, &open_options);
            });
            if !watcher.start() {
                let listed: Vec<String> = roots.iter().map(FileUrl::path).collect();
                write_error(&format!("could not watch {}", listed.join(", ")), 70);
            }
            let scope: Vec<String> = roots.iter().map(FileUrl::last_path_component).collect();
            write_stderr(
                format!("down: watching {} — Markdown changes open in Downright. ^C to stop.\n", scope.join(", ")).as_bytes(),
            );
            flush_printed();
            std::mem::forget(watcher);
            dispatch2::dispatch_main();
        }
        Action::Hook(options) => {
            let executable = resolved_executable();
            match options.mode {
                HookMode::Print => print(&agent_bridge::hook_snippet(&executable)),
                HookMode::Install | HookMode::Uninstall => {
                    let url = options.scope.settings_url(
                        &FileUrl::from_path(&foundation_io::ns_home_directory()),
                        &FileUrl::from_path(&url::current_directory_path()),
                    );
                    let existing = match markdown_cli::load_settings_default(&url) {
                        Ok(existing) => existing,
                        Err(error) => write_error(&error.to_string(), 65),
                    };
                    let (settings, changed) = if options.mode == HookMode::Install {
                        agent_bridge::installing_hook(&existing, &executable)
                    } else {
                        agent_bridge::removing_hook(&existing, &executable)
                    };
                    if !changed {
                        print(&if options.mode == HookMode::Install {
                            format!("Already installed in {}", url.path())
                        } else {
                            format!("No Downright hook found in {}", url.path())
                        });
                        exit(0);
                    }
                    let written = foundation_io::create_directory(&url.deleting_last_path_component(), true)
                        .map_err(|error| error.description)
                        .and_then(|()| agent_bridge::encode(&settings))
                        .and_then(|data| foundation_io::write_atomically(&data, &url).map_err(|error| error.description));
                    if let Err(description) = written {
                        write_error(&format!("cannot write {}: {description}", url.path()), 73);
                    }
                    print(&if options.mode == HookMode::Install {
                        format!("Installed in {} — agent edits to Markdown now open in Downright.", url.path())
                    } else {
                        format!("Removed from {}.", url.path())
                    });
                }
            }
        }
    }
    exit(0);
}

