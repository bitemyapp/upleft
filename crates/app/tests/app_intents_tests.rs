//! `OpenMarkdownIntent.perform()`'s Rust body. Downright has no tests for
//! AppIntents.swift (NativeIntegrationTests covers the policy and registry,
//! which `integrations::native_integration` ports); these pin the routing and
//! the error texts. `perform` opens on the main queue, so this binary owns
//! the main thread.

mod main_thread;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use objc2::MainThreadMarker;
use upleft_app::integrations::app_intents::*;
use upleft_foundation::url::FileUrl;

static OPENED: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn normalized_path(path: &str) -> Option<FileUrl> {
    path.ends_with(".md").then(|| FileUrl::from_path(path))
}

fn open(url: &FileUrl, _mtm: MainThreadMarker) -> bool {
    OPENED.lock().unwrap().push(url.path());
    !url.path().contains("refused")
}

fn perform_now(path: &str) -> Result<(), OpenMarkdownIntentError> {
    let outcome: Arc<Mutex<Option<Result<(), OpenMarkdownIntentError>>>> = Arc::default();
    let sink = outcome.clone();
    perform(path.to_owned(), move |result| {
        *sink.lock().unwrap() = Some(result);
    });
    assert!(main_thread::pump_until(|| outcome.lock().unwrap().is_some(), Duration::from_secs(5)));
    outcome.lock().unwrap().take().unwrap()
}

fn without_a_registry_the_intent_is_unavailable() {
    assert_eq!(perform_now("/tmp/readme.md"), Err(OpenMarkdownIntentError::Unavailable));
}

fn perform_routes_through_policy_then_registry() {
    install_native_integration(NativeIntegrationCalls {
        native_integration_policy_normalized_path: normalized_path,
        integration_registry_shared_open: open,
    });
    assert_eq!(perform_now("/tmp/notes.txt"), Err(OpenMarkdownIntentError::UnsupportedFile));
    assert!(OPENED.lock().unwrap().is_empty(), "an unsupported file never reaches the registry");
    assert_eq!(perform_now("/tmp/upleft-intent/readme.md"), Ok(()));
    assert_eq!(perform_now("/tmp/upleft-intent/refused.md"), Err(OpenMarkdownIntentError::Unavailable));
    assert_eq!(
        *OPENED.lock().unwrap(),
        vec!["/tmp/upleft-intent/readme.md".to_owned(), "/tmp/upleft-intent/refused.md".to_owned()]
    );
}

fn errors_describe_themselves_as_downright_does() {
    assert_eq!(OpenMarkdownIntentError::UnsupportedFile.error_description(), Some("Choose a Markdown file."));
    assert_eq!(OpenMarkdownIntentError::Unavailable.error_description(), Some("Downright could not open that file."));
    assert_eq!(OPEN_MARKDOWN_INTENT_TITLE, "Open Markdown in Downright");
    assert!(declarations_available(), "the shim is built against an SDK with AppIntents");
    register();
}

fn main() {
    main_thread::run(&[
        ("without_a_registry_the_intent_is_unavailable", without_a_registry_the_intent_is_unavailable),
        ("perform_routes_through_policy_then_registry", perform_routes_through_policy_then_registry),
        ("errors_describe_themselves_as_downright_does", errors_describe_themselves_as_downright_does),
    ]);
}
