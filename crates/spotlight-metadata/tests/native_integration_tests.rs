//! The Spotlight test of `Tests/DownrightAppTests/NativeIntegrationTests.swift`
//! (`spotlightMetadataUsesFrontMatterAndHeading`). The app re-exports these
//! types (`upleft_app::integrations::spotlight_metadata`), so the test runs
//! against the shared crate. The file's other two tests
//! (`policyAcceptsOnlyMarkdownFiles`, `registryNormalizesAndRoutesExistingFile`)
//! cover `NativeIntegration.swift` and belong to that module's port.

use upleft_foundation::url::FileUrl;
use upleft_spotlight_metadata::spotlight_metadata::{AttributeValue, SpotlightMetadataImporter, SpotlightMetadataKey};

#[test]
fn spotlight_metadata_uses_front_matter_and_heading() {
    let url = FileUrl::from_path("/tmp/guide.md");
    let text = "---\ntags: [agents, markdown]\n---\n\n# Guide\n\nBody.\n";
    let metadata = SpotlightMetadataImporter::metadata_for_text(text, &url);
    assert_eq!(metadata.title, "Guide");
    assert_eq!(metadata.keywords, vec!["agents", "markdown"]);
    assert_eq!(
        metadata.attribute(SpotlightMetadataKey::KIND),
        Some(AttributeValue::String("Markdown document".into()))
    );
}
