//! Port of `Sources/drdownright/Doctor.swift`.
//!
//! A diagnostic result that is safe to print in a terminal or serialize for a
//! support report. The doctor never repairs the machine; it only reports what
//! is installed and which integration contract is missing.
//!
//! The same external tools are run the same way (`codesign`, `spctl`,
//! `xcrun stapler`, `pluginkit`, stdout and stderr into one pipe), the
//! property list is read by `PropertyListSerialization`, and the default
//! application comes from `NSWorkspace`, so a report matches Downright's on
//! the same machine.

use std::io::Read;
use std::os::unix::process::ExitStatusExt;
use std::process::{Command, Stdio};

use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::AnyObject;
use objc2_app_kit::NSWorkspace;
use objc2_foundation::{NSArray, NSData, NSDictionary, NSPropertyListReadOptions, NSPropertyListSerialization, NSString};
use objc2_uniform_type_identifiers::{UTType, UTTypeData};
use upleft_foundation::foundation_io;
use upleft_foundation::json_encoder::{self, JsonValue, OutputFormatting};
use upleft_foundation::url::{self, FileUrl};
use upleft_swift_text::{self as swift_text, CharSet};

/// `DoctorStatus`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DoctorStatus {
    Pass,
    Warning,
    Failure,
    Unavailable,
}

impl DoctorStatus {
    pub fn raw_value(&self) -> &'static str {
        match self {
            DoctorStatus::Pass => "pass",
            DoctorStatus::Warning => "warning",
            DoctorStatus::Failure => "failure",
            DoctorStatus::Unavailable => "unavailable",
        }
    }

    /// The private `label` extension.
    fn label(&self) -> &'static str {
        match self {
            DoctorStatus::Pass => "PASS",
            DoctorStatus::Warning => "WARN",
            DoctorStatus::Failure => "FAIL",
            DoctorStatus::Unavailable => "N/A",
        }
    }
}

/// `DoctorCheck`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DoctorCheck {
    pub id: String,
    pub status: DoctorStatus,
    pub message: String,
    pub details: Vec<String>,
}

impl DoctorCheck {
    pub fn new(id: impl Into<String>, status: DoctorStatus, message: impl Into<String>, details: Vec<String>) -> Self {
        DoctorCheck { id: id.into(), status, message: message.into(), details }
    }

    fn json_value(&self) -> JsonValue {
        JsonValue::object([
            ("id", JsonValue::String(self.id.clone())),
            ("status", JsonValue::String(self.status.raw_value().into())),
            ("message", JsonValue::String(self.message.clone())),
            ("details", JsonValue::Array(self.details.iter().cloned().map(JsonValue::String).collect())),
        ])
    }
}

/// `DoctorReport`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DoctorReport {
    pub app_path: Option<String>,
    pub version: Option<String>,
    pub checks: Vec<DoctorCheck>,
}

impl DoctorReport {
    pub fn has_failures(&self) -> bool {
        self.checks.iter().any(|check| check.status == DoctorStatus::Failure)
    }

    /// The synthesized `Codable` encoding (`encodeIfPresent` for optionals).
    fn json_value(&self) -> JsonValue {
        let mut members = Vec::new();
        JsonValue::push_if_present(&mut members, "appPath", self.app_path.clone().map(JsonValue::String));
        JsonValue::push_if_present(&mut members, "version", self.version.clone().map(JsonValue::String));
        members.push(("checks".into(), JsonValue::Array(self.checks.iter().map(DoctorCheck::json_value).collect())));
        JsonValue::Object(members)
    }
}

struct CommandResult {
    status: i32,
    output: String,
}

const APP_NAME: &str = "Upleft.app";
const BUNDLE_IDENTIFIER: &str = "com.bitemyapp.upleft";
const PREVIEW_IDENTIFIER: &str = "com.bitemyapp.upleft.quicklook";
const THUMBNAIL_IDENTIFIER: &str = "com.bitemyapp.upleft.thumbnail";
const MARKDOWN_EXTENSIONS: [&str; 8] = ["md", "markdown", "mdown", "mkd", "mdx", "mdc", "qmd", "rmd"];

fn command_directories() -> Vec<String> {
    vec![
        "/usr/local/bin".into(),
        "/opt/homebrew/bin".into(),
        foundation_io::home_directory_for_current_user().appending_path_component(".local/bin").path(),
    ]
}

/// A property-list value, as far as `as? String`, `as? [String]` and
/// `as? [[String: Any]]` can tell them apart.
#[derive(Clone, Debug, PartialEq)]
pub enum Plist {
    String(String),
    Array(Vec<Plist>),
    Dictionary(Vec<(String, Plist)>),
    Other,
}

impl Plist {
    fn as_str(&self) -> Option<&str> {
        match self {
            Plist::String(text) => Some(text),
            _ => None,
        }
    }

    fn get(&self, key: &str) -> Option<&Plist> {
        match self {
            Plist::Dictionary(members) => members.iter().find(|(k, _)| k == key).map(|(_, value)| value),
            _ => None,
        }
    }

    /// `as? [[String: Any]]`.
    fn as_dictionary_array(&self) -> Option<Vec<&Plist>> {
        match self {
            Plist::Array(values) => values.iter().map(|value| matches!(value, Plist::Dictionary(_)).then_some(value)).collect(),
            _ => None,
        }
    }

    /// `as? [String]`.
    fn as_string_array(&self) -> Option<Vec<&str>> {
        match self {
            Plist::Array(values) => values.iter().map(Plist::as_str).collect(),
            _ => None,
        }
    }

    fn from_foundation(object: &AnyObject) -> Plist {
        if let Some(string) = object.downcast_ref::<NSString>() {
            return Plist::String(swift_text::ns::foundation::to_string(string));
        }
        if let Some(array) = object.downcast_ref::<NSArray>() {
            return Plist::Array(array.iter().map(|value| Plist::from_foundation(&value)).collect());
        }
        if let Some(dictionary) = object.downcast_ref::<NSDictionary>() {
            let mut members = Vec::new();
            for key in dictionary.allKeys() {
                // `as? [String: Any]` fails on a non-string key.
                let Some(name) = key.downcast_ref::<NSString>() else { return Plist::Other };
                let value = dictionary.objectForKey(&key).map(|value| Plist::from_foundation(&value)).unwrap_or(Plist::Other);
                members.push((swift_text::ns::foundation::to_string(name), value));
            }
            return Plist::Dictionary(members);
        }
        Plist::Other
    }
}

/// `DownDoctor`.
pub struct DownDoctor;

impl DownDoctor {
    /// Runs every diagnostic without making a repair or asking for a password.
    pub fn run(app_path: Option<&str>) -> DoctorReport {
        let Some(app) = resolve_app(app_path) else {
            return DoctorReport {
                app_path: None,
                version: None,
                checks: vec![DoctorCheck::new(
                    "installed-app",
                    DoctorStatus::Failure,
                    "Upleft.app was not found in /Applications, ~/Applications, or the current bundle.",
                    vec!["Install the signed app, then run down doctor again.".into()],
                )],
            };
        };

        let info = info_dictionary(&app);
        let version = info.as_ref().and_then(|info| info.get("CFBundleShortVersionString")).and_then(Plist::as_str).map(str::to_owned);
        let mut checks = Vec::new();

        checks.push(DoctorCheck::new(
            "installed-app",
            DoctorStatus::Pass,
            format!("Found {}", app.path()),
            version.iter().map(|version| format!("Version {version}")).collect(),
        ));
        checks.push(bundle_check(&app, info.as_ref()));
        checks.push(application_location_check(&app));
        checks.push(signature_check(&app));
        checks.push(gatekeeper_check(&app));
        checks.push(notarization_check(&app));
        checks.push(command_line_check(&app));
        checks.push(plugin_check(&app, "Quick Look preview", PREVIEW_IDENTIFIER, "DownrightQL.appex"));
        checks.push(plugin_check(&app, "Finder thumbnail", THUMBNAIL_IDENTIFIER, "DownrightThumb.appex"));
        checks.push(default_association_check(&app));
        checks.push(uti_check(info.as_ref()));
        checks.push(update_channel_check(info.as_ref()));

        DoctorReport { app_path: Some(app.path()), version, checks }
    }

    /// `DownDoctor.json(_:)`: `JSONEncoder` with `[.prettyPrinted, .sortedKeys]`.
    pub fn json(report: &DoctorReport) -> String {
        json_encoder::encode_string(
            &report.json_value(),
            OutputFormatting { pretty_printed: true, sorted_keys: true, ..Default::default() },
        )
    }

    pub fn human_readable(report: &DoctorReport) -> String {
        let mut lines = vec!["Upleft doctor".to_owned()];
        if let Some(app_path) = &report.app_path {
            lines.push(format!("App: {app_path}"));
        }
        if let Some(version) = &report.version {
            lines.push(format!("Version: {version}"));
        }
        lines.push(String::new());
        for check in &report.checks {
            lines.push(format!("[{}] {}: {}", check.status.label(), check.id, check.message));
            lines.extend(check.details.iter().map(|detail| format!("  - {detail}")));
        }
        lines.join("\n")
    }

    /// Kept public so the parser semantics can be pinned without invoking
    /// LaunchServices or pluginkit in a test process.
    pub fn plugin_is_enabled(listing: &str, identifier: &str) -> bool {
        let Some(line) = swift_text::split_default(listing, '\n')
            .into_iter()
            .find(|line| swift_text::contains(line, identifier))
        else {
            return false;
        };
        !swift_text::has_prefix(swift_text::trimming(line, CharSet::Whitespaces), "-")
    }
}

/// `CommandLine.arguments.first ?? ""`.
fn argv0() -> String {
    std::env::args_os().next().map(|argument| argument.to_string_lossy().into_owned()).unwrap_or_default()
}

fn resolve_app(explicit_path: Option<&str>) -> Option<FileUrl> {
    let current_bundle = FileUrl::from_path(&argv0())
        .standardized_file_url()
        .deleting_last_path_component()
        .deleting_last_path_component()
        .deleting_last_path_component();
    let mut candidates = Vec::new();
    if let Some(explicit) = explicit_path {
        candidates.push(FileUrl::from_path(&url::expanding_tilde_in_path(explicit)));
    }
    candidates.push(FileUrl::from_path(&format!("/Applications/{APP_NAME}")));
    candidates.push(foundation_io::home_directory_for_current_user().appending_path_component(&format!("Applications/{APP_NAME}")));
    if current_bundle.path_extension() == "app" {
        candidates.push(current_bundle);
    }
    candidates.into_iter().find(|candidate| foundation_io::file_exists(&candidate.path()))
}

fn info_dictionary(app: &FileUrl) -> Option<Plist> {
    let info_url = app.appending_path_component("Contents/Info.plist");
    let data = std::fs::read(info_url.path()).ok()?;
    autoreleasepool(|_| {
        let object = unsafe {
            NSPropertyListSerialization::propertyListWithData_options_format_error(
                &NSData::with_bytes(&data),
                NSPropertyListReadOptions::empty(),
                std::ptr::null_mut(),
            )
        }
        .ok()?;
        let info = Plist::from_foundation(&object);
        matches!(info, Plist::Dictionary(_)).then_some(info)
    })
}

fn bundle_check(app: &FileUrl, info: Option<&Plist>) -> DoctorCheck {
    let Some(info) = info else {
        return DoctorCheck::new("bundle", DoctorStatus::Failure, "Info.plist could not be read.", vec![]);
    };
    let identifier = info.get("CFBundleIdentifier").and_then(Plist::as_str);
    let executable = app.appending_path_component("Contents/MacOS/Upleft");
    if !identifier.is_some_and(|identifier| swift_text::str_eq(identifier, BUNDLE_IDENTIFIER)) {
        return DoctorCheck::new(
            "bundle",
            DoctorStatus::Failure,
            format!("Bundle identifier is {}, expected {BUNDLE_IDENTIFIER}.", identifier.unwrap_or("missing")),
            vec![],
        );
    }
    if !foundation_io::is_executable_file(&executable.path()) {
        return DoctorCheck::new("bundle", DoctorStatus::Failure, "The host executable is missing or not executable.", vec![]);
    }
    DoctorCheck::new("bundle", DoctorStatus::Pass, "Bundle identity and host executable are present.", vec![])
}

fn application_location_check(app: &FileUrl) -> DoctorCheck {
    let path = app.resolving_symlinks_in_path().path();
    let home_applications =
        foundation_io::home_directory_for_current_user().appending_path_component("Applications").path() + "/";
    if swift_text::has_prefix(&path, "/Applications/") || swift_text::has_prefix(&path, &home_applications) {
        return DoctorCheck::new("application-location", DoctorStatus::Pass, "Bundle is in an Applications folder.", vec![]);
    }
    if swift_text::contains(&path, "/AppTranslocation/") {
        return DoctorCheck::new(
            "application-location",
            DoctorStatus::Failure,
            "Bundle is running from App Translocation; Quick Look and CLI registration will not persist.",
            vec!["Move Upleft.app to /Applications and run doctor again.".into()],
        );
    }
    DoctorCheck::new(
        "application-location",
        DoctorStatus::Warning,
        "Bundle is outside an Applications folder.",
        vec!["A development bundle can work, but system integrations may not register permanently.".into()],
    )
}

/// `[text].filter { !$0.isEmpty }` after trimming.
fn trimmed_output(output: &str) -> String {
    swift_text::trimming(output, CharSet::WhitespacesAndNewlines).to_owned()
}

fn non_empty(values: Vec<String>) -> Vec<String> {
    values.into_iter().filter(|value| !value.is_empty()).collect()
}

fn signature_check(app: &FileUrl) -> DoctorCheck {
    let Some(result) = run("/usr/bin/codesign", &["--verify", "--deep", "--strict", "--verbose=2", &app.path()]) else {
        return DoctorCheck::new("code-signature", DoctorStatus::Unavailable, "codesign is not available on this machine.", vec![]);
    };
    if result.status == 0 {
        DoctorCheck::new("code-signature", DoctorStatus::Pass, "Code signature verifies.", vec![])
    } else {
        DoctorCheck::new(
            "code-signature",
            DoctorStatus::Failure,
            "Code signature verification failed.",
            non_empty(vec![trimmed_output(&result.output)]),
        )
    }
}

fn gatekeeper_check(app: &FileUrl) -> DoctorCheck {
    let Some(result) = run("/usr/sbin/spctl", &["--assess", "--type", "execute", "--verbose=4", &app.path()]) else {
        return DoctorCheck::new("gatekeeper", DoctorStatus::Unavailable, "spctl is not available on this machine.", vec![]);
    };
    if result.status == 0 {
        DoctorCheck::new("gatekeeper", DoctorStatus::Pass, "Gatekeeper accepts the app.", vec![])
    } else {
        DoctorCheck::new(
            "gatekeeper",
            DoctorStatus::Warning,
            "Gatekeeper did not accept this bundle.",
            non_empty(vec![trimmed_output(&result.output)]),
        )
    }
}

fn notarization_check(app: &FileUrl) -> DoctorCheck {
    let Some(result) = run("/usr/bin/xcrun", &["stapler", "validate", &app.path()]) else {
        return DoctorCheck::new(
            "notarization",
            DoctorStatus::Unavailable,
            "xcrun stapler is not available on this machine.",
            vec![],
        );
    };
    if result.status == 0 {
        DoctorCheck::new("notarization", DoctorStatus::Pass, "Notarization ticket validates.", vec![])
    } else {
        DoctorCheck::new(
            "notarization",
            DoctorStatus::Warning,
            "No valid stapled notarization ticket was found.",
            non_empty(vec!["This is expected for an unsigned development bundle.".into(), trimmed_output(&result.output)]),
        )
    }
}

fn command_line_check(app: &FileUrl) -> DoctorCheck {
    let expected = app.appending_path_component("Contents/MacOS/down").resolving_symlinks_in_path().path();
    let mut matches = Vec::new();
    let mut foreign = Vec::new();
    for directory in command_directories() {
        for name in ["down", "md"] {
            let link = FileUrl::from_path(&directory).appending_path_component(name);
            let Some(destination) = foundation_io::destination_of_symbolic_link(&link.path()) else { continue };
            let resolved = FileUrl::from_path_relative_to(&destination, &link.deleting_last_path_component())
                .resolving_symlinks_in_path()
                .path();
            if swift_text::str_eq(&resolved, &expected) {
                matches.push(link.path());
            } else {
                foreign.push(link.path());
            }
        }
    }
    if matches.is_empty() {
        let mut details: Vec<String> = foreign.iter().map(|path| format!("Foreign or stale alias: {path}")).collect();
        details.push("Install the CLI from Upleft's setup panel to repair it.".into());
        return DoctorCheck::new("cli-path", DoctorStatus::Warning, "No down/md alias points to this app.", details);
    }
    DoctorCheck::new("cli-path", DoctorStatus::Pass, "CLI aliases point to this app.", matches)
}

fn plugin_check(app: &FileUrl, name: &str, identifier: &str, bundle_name: &str) -> DoctorCheck {
    let path = app.appending_path_component(&format!("Contents/PlugIns/{bundle_name}"));
    if !foundation_io::file_exists(&path.path()) {
        return DoctorCheck::new(
            identifier,
            DoctorStatus::Warning,
            format!("{name} extension is not bundled."),
            vec!["The SwiftPM development bundle cannot provide the .appex integration.".into()],
        );
    }
    let Some(result) = run("/usr/bin/pluginkit", &["-m", "-v", "-i", identifier]) else {
        return DoctorCheck::new(identifier, DoctorStatus::Unavailable, "pluginkit is not available on this machine.", vec![]);
    };
    if !DownDoctor::plugin_is_enabled(&result.output, identifier) {
        return DoctorCheck::new(
            identifier,
            DoctorStatus::Warning,
            format!("{name} is bundled but not enabled by pluginkit."),
            vec!["Run the app's setup integration or reopen the signed app in /Applications.".into()],
        );
    }
    DoctorCheck::new(identifier, DoctorStatus::Pass, format!("{name} is bundled and enabled."), vec![])
}

/// `UTType(filenameExtension: "md")` (conforming to `.data`) and
/// `NSWorkspace.shared.urlForApplication(toOpen:)`.
fn default_markdown_handler() -> Option<FileUrl> {
    autoreleasepool(|_| {
        let markdown =
            UTType::typeWithFilenameExtension_conformingToType(&NSString::from_str("md"), unsafe { UTTypeData })?;
        let handler: Retained<objc2_foundation::NSURL> =
            NSWorkspace::sharedWorkspace().URLForApplicationToOpenContentType(&markdown)?;
        FileUrl::from_nsurl(&handler)
    })
}

fn default_association_check(app: &FileUrl) -> DoctorCheck {
    let Some(handler) = default_markdown_handler() else {
        return DoctorCheck::new(
            "default-markdown-app",
            DoctorStatus::Warning,
            "macOS has no default application for .md files.",
            vec![],
        );
    };
    let expected = app.resolving_symlinks_in_path().standardized_file_url();
    let actual = handler.resolving_symlinks_in_path().standardized_file_url();
    if actual == expected {
        return DoctorCheck::new("default-markdown-app", DoctorStatus::Pass, "Upleft is the default .md application.", vec![]);
    }
    DoctorCheck::new(
        "default-markdown-app",
        DoctorStatus::Warning,
        format!("macOS currently opens .md files with {}.", handler.last_path_component()),
        vec!["The app still supports Open With; choose Upleft if you want it as the default.".into()],
    )
}

fn uti_check(info: Option<&Plist>) -> DoctorCheck {
    let document_types =
        info.and_then(|info| info.get("CFBundleDocumentTypes")).and_then(Plist::as_dictionary_array).unwrap_or_default();
    let extensions: Vec<String> = document_types
        .iter()
        .flat_map(|document_type| {
            document_type.get("CFBundleTypeExtensions").and_then(Plist::as_string_array).unwrap_or_default()
        })
        .map(swift_text::lowercased)
        .collect();
    let missing: Vec<&str> = MARKDOWN_EXTENSIONS
        .iter()
        .copied()
        .filter(|extension| !extensions.iter().any(|declared| swift_text::str_eq(declared, extension)))
        .collect();
    if missing.is_empty() {
        return DoctorCheck::new(
            "markdown-types",
            DoctorStatus::Pass,
            "All advertised Markdown extensions are declared.",
            MARKDOWN_EXTENSIONS.iter().map(|extension| format!(".{extension}")).collect(),
        );
    }
    DoctorCheck::new(
        "markdown-types",
        DoctorStatus::Failure,
        "Bundle document types are missing advertised extensions.",
        missing.iter().map(|extension| format!(".{extension}")).collect(),
    )
}

fn update_channel_check(info: Option<&Plist>) -> DoctorCheck {
    let feed = info.and_then(|info| info.get("SUFeedURL")).and_then(Plist::as_str);
    let key = info.and_then(|info| info.get("SUPublicEDKey")).and_then(Plist::as_str);
    let Some(feed) = feed.filter(|feed| !feed.is_empty()) else {
        return DoctorCheck::new(
            "update-channel",
            DoctorStatus::Warning,
            "Sparkle update feed is not configured in this bundle.",
            vec![],
        );
    };
    if !key.is_some_and(|key| !key.is_empty() && !swift_text::contains(key, "PLACEHOLDER")) {
        return DoctorCheck::new(
            "update-channel",
            DoctorStatus::Warning,
            "Sparkle feed exists but its EdDSA public key is still a placeholder.",
            vec![feed.to_owned()],
        );
    }
    DoctorCheck::new("update-channel", DoctorStatus::Pass, "Sparkle feed and public key are configured.", vec![feed.to_owned()])
}

/// `DownDoctor.run(_:_:)`: `Process` with stdout and stderr on one pipe.
/// `terminationStatus` is the exit code, or the signal that ended the tool.
fn run(launch_path: &str, arguments: &[&str]) -> Option<CommandResult> {
    if !foundation_io::is_executable_file(launch_path) {
        return None;
    }
    let (mut reader, writer) = std::io::pipe().ok()?;
    let error_writer = writer.try_clone().ok()?;
    let mut child = Command::new(launch_path)
        .args(arguments)
        .stdout(Stdio::from(writer))
        .stderr(Stdio::from(error_writer))
        .spawn()
        .ok()?;
    let mut data = Vec::new();
    let _ = reader.read_to_end(&mut data);
    let status = child.wait().ok()?;
    let status = status.code().or(status.signal()).unwrap_or(0);
    Some(CommandResult { status, output: String::from_utf8_lossy(&data).into_owned() })
}
