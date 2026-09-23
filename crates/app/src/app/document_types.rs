//! Port of `Sources/DownrightApp/App/DocumentTypes.swift`.
//!
//! The UTIs from §10, in one place so the open panel, the drag destination,
//! and the Quick Look extension can never disagree about what this app opens.

use objc2::rc::Retained;
use objc2::Message;
use objc2_foundation::NSObjectProtocol;
use objc2_foundation::NSString;
use objc2_uniform_type_identifiers::UTType;
use upleft_foundation::url::FileUrl;

pub const FILE_EXTENSIONS: [&str; 8] = ["md", "markdown", "mdown", "mkd", "mdx", "mdc", "qmd", "rmd"];

/// `DocumentTypes.contentTypes`.
pub fn content_types() -> Vec<Retained<UTType>> {
    let mut types: Vec<Retained<UTType>> = Vec::new();
    if let Some(markdown) = UTType::typeWithIdentifier(&NSString::from_str("net.daringfireball.markdown")) {
        types.push(markdown);
    }
    for extension in FILE_EXTENSIONS {
        if let Some(kind) = UTType::typeWithFilenameExtension(&NSString::from_str(extension))
            && !types.iter().any(|existing| existing.isEqual(Some(&*kind)))
        {
            types.push(kind);
        }
    }
    types.push(unsafe { objc2_uniform_type_identifiers::UTTypePlainText }.retain());
    types
}

/// `DocumentTypes.isMarkdown(_:)`: `pathExtension.lowercased()` against the list.
pub fn is_markdown(path_extension: &str) -> bool {
    let lowered = upleft_swift_text::lowercased(path_extension);
    FILE_EXTENSIONS.contains(&lowered.as_str())
}

/// Extensions LaunchServices *executes* when asked to "open" them, rather
/// than presenting a document: application bundles and Terminal-run
/// scripts. A path link or token resolving to one of these must never
/// hand the target to `NSWorkspace.open` — the documented contract is
/// "open in your editor", not "run whatever this document points at".
const EXECUTABLE_EXTENSIONS: [&str; 14] = [
    "app", "bundle", "appex", "xpc", "plugin", "kext", "prefpane", "qlgenerator", "workflow", "action", "command",
    "term", "terminal", "tool",
];

/// Whether opening `url` through LaunchServices would run code instead of
/// showing a document. Directories are judged by their bundle extension;
/// plain files by a known executing extension or an executable bit with
/// no document extension at all (an extension-less compiled tool or
/// `chmod +x` script).
pub fn executes_when_opened(url: &FileUrl) -> bool {
    let path = url.path();
    let Ok(metadata) = std::fs::metadata(&path) else {
        return false;
    };
    let path_extension = upleft_swift_text::lowercased(&url.path_extension());
    if metadata.is_dir() {
        return EXECUTABLE_EXTENSIONS.contains(&path_extension.as_str());
    }
    if EXECUTABLE_EXTENSIONS.contains(&path_extension.as_str()) {
        return true;
    }
    // An executable-bit file with no extension at all is a raw binary or
    // a chmod'ed script; either way "open" means run.
    path_extension.is_empty()
        && objc2_foundation::NSFileManager::defaultManager().isExecutableFileAtPath(&NSString::from_str(&path))
}
