//! `OpenMarkdownIntent.perform()`'s Rust body. Downright has no tests for
//! AppIntents.swift (NativeIntegrationTests covers the policy and registry,
//! which `integrations::native_integration` ports); these pin the routing and
//! the error texts. `perform` opens on the main queue, so this binary owns
//! the main thread.

mod main_thread;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use objc2::MainThreadMarker;
use upleft_app::integrations::app_intents::*;
use upleft_app::integrations::native_integration::{IntegrationRegistry, NativeIntegrationPolicy, OpenHandler};
use upleft_foundation::url::FileUrl;

fn fixture() -> std::path::PathBuf {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let directory = std::env::temp_dir().join(format!(
        "upleft-intent-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&directory).unwrap();
    directory
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

fn without_an_open_handler_the_intent_is_unavailable() {
    let directory = fixture();
    let file = directory.join("readme.md");
    std::fs::write(&file, "# Readme\n").unwrap();
    assert_eq!(perform_now(file.to_str().unwrap()), Err(OpenMarkdownIntentError::Unavailable));
    let _ = std::fs::remove_dir_all(directory);
}

fn perform_routes_through_policy_then_registry() {
    let mtm = MainThreadMarker::new().unwrap();
    let opened: Rc<RefCell<Vec<String>>> = Rc::default();
    let sink = opened.clone();
    let handler: OpenHandler = Rc::new(move |url: &FileUrl| sink.borrow_mut().push(url.path()));
    IntegrationRegistry::shared(mtm).set_open_handler(Some(handler));

    let directory = fixture();
    let readme = directory.join("readme.md");
    std::fs::write(&readme, "# Readme\n").unwrap();
    let notes = directory.join("notes.txt");
    std::fs::write(&notes, "notes\n").unwrap();

    assert_eq!(perform_now(notes.to_str().unwrap()), Err(OpenMarkdownIntentError::UnsupportedFile));
    assert!(opened.borrow().is_empty(), "an unsupported file never reaches the registry");
    assert_eq!(perform_now(readme.to_str().unwrap()), Ok(()));
    let missing = directory.join("missing.md");
    assert_eq!(perform_now(missing.to_str().unwrap()), Err(OpenMarkdownIntentError::Unavailable));
    let expected = NativeIntegrationPolicy::normalized_path(readme.to_str().unwrap()).unwrap().path();
    assert_eq!(*opened.borrow(), vec![expected]);

    IntegrationRegistry::shared(mtm).set_open_handler(None);
    let _ = std::fs::remove_dir_all(directory);
}

fn errors_describe_themselves_as_downright_does() {
    assert_eq!(OpenMarkdownIntentError::UnsupportedFile.error_description(), Some("Choose a Markdown file."));
    assert_eq!(OpenMarkdownIntentError::Unavailable.error_description(), Some("Upleft could not open that file."));
    assert_eq!(OPEN_MARKDOWN_INTENT_TITLE, "Open Markdown in Upleft");
    assert!(declarations_available(), "the shim is built against an SDK with AppIntents");
    register();
}

fn main() {
    main_thread::run(&[
        ("without_an_open_handler_the_intent_is_unavailable", without_an_open_handler_the_intent_is_unavailable),
        ("perform_routes_through_policy_then_registry", perform_routes_through_policy_then_registry),
        ("errors_describe_themselves_as_downright_does", errors_describe_themselves_as_downright_does),
    ]);
}
