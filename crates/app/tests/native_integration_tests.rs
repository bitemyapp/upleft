//! Port of `Tests/DownrightAppTests/NativeIntegrationTests.swift`.
//!
//! Skipped: spotlightMetadataUsesFrontMatterAndHeading tests
//! `SpotlightMetadataImporter` (`Sources/DownrightSpotlightMetadata`), which is
//! not ported on this branch.

use std::cell::RefCell;
use std::rc::Rc;

use objc2_foundation::{NSString, NSURL};
use upleft_app::integrations::native_integration::{IntegrationRegistry, NativeIntegrationPolicy};
use upleft_foundation::url::FileUrl;

#[test]
fn policy_accepts_only_markdown_files() {
    assert!(NativeIntegrationPolicy::accepts(&FileUrl::from_path("/tmp/readme.md")));
    assert!(NativeIntegrationPolicy::accepts(&FileUrl::from_path("/tmp/NOTE.MDX")));
    assert!(!NativeIntegrationPolicy::accepts(&FileUrl::from_path("/tmp/readme.txt")));
    let web = NSURL::URLWithString(&NSString::from_str("https://example.com/readme.md")).unwrap();
    assert!(!NativeIntegrationPolicy::accepts_url(&web));
}

#[test]
fn registry_normalizes_and_routes_existing_file() {
    let temporary = objc2_foundation::NSTemporaryDirectory().to_string();
    let url = FileUrl::from_path(&temporary)
        .appending_path_component(&format!("downright-integration-{}.md", objc2_foundation::NSUUID::new().UUIDString()));
    std::fs::write(url.path(), "# Title\n").unwrap();

    let routed: Rc<RefCell<Option<FileUrl>>> = Rc::new(RefCell::new(None));
    let sink = Rc::clone(&routed);
    let registry = IntegrationRegistry::new(Some(Rc::new(move |url: &FileUrl| *sink.borrow_mut() = Some(url.clone()))));
    let opened = registry.open(&url.standardized_file_url());
    let routed_url = routed.borrow().clone();
    let text_opened = registry.open(&url.deleting_path_extension().appending_path_extension("txt"));
    let _ = std::fs::remove_file(url.path());

    assert!(opened);
    assert_eq!(routed_url, Some(url.standardized_file_url()));
    assert!(!text_opened);
}
