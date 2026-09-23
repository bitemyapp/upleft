//! Port of `Tests/DownrightAppTests/SystemIntegrationTests.swift`.
//!
//! Skipped: the three `TourRetirementTests` (offeredEarly, retiredByLaunchCount,
//! retiredOnceTaken) test `AppDelegate.shouldOfferTour`, which belongs to
//! `App/AppDelegate.swift` and arrives with the UI port.

use objc2_foundation::NSObjectProtocol;
use upleft_app::app::document_types;
use upleft_app::support::system_integration::SystemIntegration;

// MARK: - pluginkit flag semantics
//
// Real `pluginkit -m -v -i …` output captured on macOS: blank means
// *available*, and testing for a leading "+" reports every healthy install as
// broken.

/// A registered, working extension. Five leading spaces, no flag.
const AVAILABLE_LISTING: &str = "     com.ezzy.downright.quicklook(1.0)\t6DFDA989-A19F-510C-999F-175575BC16CF\t2026-07-23 03:00:13 +0000\t/Applications/Downright.app/Contents/PlugIns/DownrightQL.appex\n (1 plug-in)";

/// The user switched it on by hand in System Settings.
const EXPLICITLY_ENABLED_LISTING: &str = "+    com.ezzy.downright.quicklook(1.0)\tUUID\t2026-07-23 03:00:13 +0000\t/Applications/Downright.app/Contents/PlugIns/DownrightQL.appex\n (1 plug-in)";

/// The user switched it off.
const DISABLED_LISTING: &str = "-    com.ezzy.downright.quicklook(1.0)\tUUID\t2026-07-23 03:00:13 +0000\t/Applications/Downright.app/Contents/PlugIns/DownrightQL.appex\n (1 plug-in)";

/// Never registered. `pluginkit` exits 0 for this.
const NO_MATCHES_LISTING: &str = "  (no matches)\n";

// Quick Look extension state

#[test]
fn blank_flag_is_enabled() {
    assert!(SystemIntegration::is_enabled(AVAILABLE_LISTING, SystemIntegration::PREVIEW_EXTENSION_IDENTIFIER));
}

#[test]
fn plus_flag_is_enabled() {
    assert!(SystemIntegration::is_enabled(EXPLICITLY_ENABLED_LISTING, SystemIntegration::PREVIEW_EXTENSION_IDENTIFIER));
}

#[test]
fn minus_flag_is_disabled() {
    assert!(!SystemIntegration::is_enabled(DISABLED_LISTING, SystemIntegration::PREVIEW_EXTENSION_IDENTIFIER));
}

#[test]
fn no_matches_is_disabled() {
    assert!(!SystemIntegration::is_enabled(NO_MATCHES_LISTING, SystemIntegration::PREVIEW_EXTENSION_IDENTIFIER));
}

#[test]
fn unrelated_listing_is_disabled() {
    let other = "     com.apple.HydraQLPreviewExtension(1.0)\tUUID\n (1 plug-in)";
    assert!(!SystemIntegration::is_enabled(other, SystemIntegration::PREVIEW_EXTENSION_IDENTIFIER));
}

// Default application claims

#[test]
fn claims_only_plain_markdown() {
    assert_eq!(SystemIntegration::CLAIMED_EXTENSIONS, ["md", "markdown", "mdown", "mkd"]);
    for owned in ["mdx", "mdc", "qmd", "rmd", "txt"] {
        assert!(!SystemIntegration::CLAIMED_EXTENSIONS.contains(&owned));
    }
}

#[test]
fn claims_are_declared() {
    for ext in SystemIntegration::CLAIMED_EXTENSIONS {
        assert!(document_types::FILE_EXTENSIONS.contains(&ext));
    }
}

#[test]
fn claimed_types_are_unique() {
    let types = SystemIntegration::claimed_types();
    assert!(!types.is_empty());
    for (index, kind) in types.iter().enumerate() {
        for other in &types[index + 1..] {
            assert!(!kind.isEqual(Some(other)), "{} is claimed twice", kind.identifier());
        }
    }
}

/// A test runner is not mistaken for an app bundle.
#[test]
fn test_runner_is_not_an_app_bundle() {
    assert!(!SystemIntegration::is_app_bundle());
}
