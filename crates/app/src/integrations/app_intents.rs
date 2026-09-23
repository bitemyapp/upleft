//! Port of `Sources/DownrightApp/Integrations/AppIntents.swift`.
//!
//! App Intents are Swift-only (macros, property wrappers, result builders),
//! so the declarations themselves, `OpenMarkdownIntent` and
//! `DownrightShortcuts`, live in the Swift shim
//! (`swift-shim/Sources/UpleftSwiftShim/AppIntentsShim.swift`, copied from
//! Downright). Their `perform()` calls back into [`perform`] here through the
//! C function pointer [`register`] installs; this module owns the body:
//! normalise the path, then open it on the main thread.
//!
//! `NativeIntegrationPolicy` and `IntegrationRegistry` belong to
//! `integrations::native_integration`, ported separately. Until the app
//! wires them in with [`install_native_integration`], `perform` answers as
//! Downright does when the registry has no open handler: `.unavailable`.

use std::ffi::c_void;
use std::sync::Mutex;

use dispatch2::DispatchQueue;
use objc2::MainThreadMarker;
use upleft_foundation::url::FileUrl;

/// `OpenMarkdownIntent.title`.
pub const OPEN_MARKDOWN_INTENT_TITLE: &str = "Open Markdown in Downright";
/// `OpenMarkdownIntent.description`.
pub const OPEN_MARKDOWN_INTENT_DESCRIPTION: &str = "Open a Markdown document in Downright.";
/// `OpenMarkdownIntent.openAppWhenRun`.
pub const OPEN_MARKDOWN_INTENT_OPENS_APP: bool = true;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenMarkdownIntentError {
    UnsupportedFile,
    Unavailable,
}

impl OpenMarkdownIntentError {
    /// `LocalizedError.errorDescription`.
    pub fn error_description(self) -> Option<&'static str> {
        match self {
            OpenMarkdownIntentError::UnsupportedFile => Some("Choose a Markdown file."),
            OpenMarkdownIntentError::Unavailable => Some("Downright could not open that file."),
        }
    }

    /// The status the shim turns back into this error.
    fn status(result: Result<(), OpenMarkdownIntentError>) -> i32 {
        match result {
            Ok(()) => 0,
            Err(OpenMarkdownIntentError::UnsupportedFile) => 1,
            Err(OpenMarkdownIntentError::Unavailable) => 2,
        }
    }
}

/// The two calls `perform()` makes into NativeIntegration.swift.
#[derive(Clone, Copy)]
pub struct NativeIntegrationCalls {
    /// `NativeIntegrationPolicy.normalizedPath(_:)`.
    pub native_integration_policy_normalized_path: fn(&str) -> Option<FileUrl>,
    /// `IntegrationRegistry.shared.open(_:)`, on the main actor.
    pub integration_registry_shared_open: fn(&FileUrl, MainThreadMarker) -> bool,
}

static NATIVE_INTEGRATION: Mutex<Option<NativeIntegrationCalls>> = Mutex::new(None);

/// Wires `perform` to `integrations::native_integration`.
pub fn install_native_integration(calls: NativeIntegrationCalls) {
    *NATIVE_INTEGRATION.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(calls);
}

/// `OpenMarkdownIntent.perform()`:
///
/// ```swift
/// guard let url = NativeIntegrationPolicy.normalizedPath(path) else {
///     throw OpenMarkdownIntentError.unsupportedFile
/// }
/// let opened = await MainActor.run { IntegrationRegistry.shared.open(url) }
/// guard opened else { throw OpenMarkdownIntentError.unavailable }
/// return .result()
/// ```
///
/// The path is normalised on the calling thread (Swift runs `perform` off
/// the main actor); the open hops to the main queue. `completion` runs once,
/// on the main thread or, for an unsupported file, on the calling thread.
pub fn perform(path: String, completion: impl FnOnce(Result<(), OpenMarkdownIntentError>) + Send + 'static) {
    let calls = *NATIVE_INTEGRATION.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(calls) = calls else {
        // No registry: `IntegrationRegistry.shared.open` has no handler and
        // returns false.
        return completion(Err(OpenMarkdownIntentError::Unavailable));
    };
    let Some(url) = (calls.native_integration_policy_normalized_path)(&path) else {
        return completion(Err(OpenMarkdownIntentError::UnsupportedFile));
    };
    DispatchQueue::main().exec_async(move || {
        let mtm = MainThreadMarker::new().expect("the main queue runs on the main thread");
        let opened = (calls.integration_registry_shared_open)(&url, mtm);
        completion(if opened { Ok(()) } else { Err(OpenMarkdownIntentError::Unavailable) });
    });
}

// MARK: - Swift shim

unsafe extern "C" {
    fn upleft_app_intents_register_open_markdown(handler: Option<OpenMarkdownHandler>);
    fn upleft_app_intents_available() -> bool;
}

type OpenMarkdownCompletion = extern "C" fn(context: *mut c_void, status: i32);
type OpenMarkdownHandler =
    extern "C" fn(path: *const u8, count: isize, context: *mut c_void, completion: OpenMarkdownCompletion);

/// The Swift continuation waiting in `perform()`.
struct PendingIntent {
    context: *mut c_void,
    completion: OpenMarkdownCompletion,
}

// SAFETY: `context` is a retained Swift continuation box, resumable from any
// thread, used exactly once.
unsafe impl Send for PendingIntent {}

extern "C" fn open_markdown(path: *const u8, count: isize, context: *mut c_void, completion: OpenMarkdownCompletion) {
    let path = if path.is_null() || count <= 0 {
        String::new()
    } else {
        // SAFETY: Swift passes `count` UTF-8 bytes, valid for this call.
        String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(path, count as usize) }).into_owned()
    };
    let pending = PendingIntent { context, completion };
    perform(path, move |result| {
        let pending = pending;
        (pending.completion)(pending.context, OpenMarkdownIntentError::status(result));
    });
}

/// Installs [`perform`] as the body of the shim's `OpenMarkdownIntent.perform()`.
/// Call once at launch.
pub fn register() {
    // SAFETY: `open_markdown` is a valid function for the life of the process.
    unsafe { upleft_app_intents_register_open_markdown(Some(open_markdown)) };
}

/// Whether the shim was built with `#if canImport(AppIntents)`, i.e. carries
/// `OpenMarkdownIntent` and `DownrightShortcuts`.
pub fn declarations_available() -> bool {
    // SAFETY: a plain query.
    unsafe { upleft_app_intents_available() }
}
