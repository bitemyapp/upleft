//! Port of `Tests/MarkdownRenderTests/LocalAssetPolicyTests.swift`: safe
//! relative image assets resolve inside the document directory; traversal,
//! absolute paths, `file://` URLs and escaping symlinks need injected trust.

use std::path::PathBuf;
use std::rc::Rc;

use objc2_foundation::NSString;
use upleft_render::fragments::fragment_base::LocalAssetAuthorizer;
use upleft_render::fragments::local_asset_policy::{LocalAssetPolicy, file_url};

struct TemporaryDirectory(PathBuf);

impl TemporaryDirectory {
    /// `FileManager.default.temporaryDirectory` plus a unique name.
    fn new() -> TemporaryDirectory {
        let base = objc2_foundation::NSTemporaryDirectory().to_string();
        // `UUID().uuidString` in Swift: unique across the parallel tests.
        let uuid = objc2_foundation::NSUUID::UUID().UUIDString().to_string();
        let path = PathBuf::from(base).join(format!("downright-image-policy-{uuid}"));
        std::fs::create_dir_all(&path).unwrap();
        TemporaryDirectory(path)
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn url_path(url: &objc2_foundation::NSURL) -> String {
    url.path().map(|path: objc2::rc::Retained<NSString>| path.to_string()).unwrap_or_default()
}

#[test]
fn safe_relative_asset() {
    let directory = TemporaryDirectory::new();
    let document = file_url(directory.0.join("notes.md").to_str().unwrap());
    let request = LocalAssetPolicy::request("images/diagram.png", Some(&document)).expect("a request");
    assert!(request.is_safe_relative);
    let expected = file_url(directory.0.join("images/diagram.png").to_str().unwrap());
    assert_eq!(url_path(&request.url), url_path(&expected));
    assert!(LocalAssetPolicy::allows(&request, None));
}

#[test]
fn unsafe_destinations_require_trust() {
    let directory = TemporaryDirectory::new();
    let document = file_url(directory.0.join("notes.md").to_str().unwrap());
    let outside = file_url(directory.0.parent().unwrap().join("secret.png").to_str().unwrap());
    let outside_path = url_path(&outside);
    let outside_string = outside.absoluteString().unwrap().to_string();
    for raw in ["../secret.png", outside_path.as_str(), outside_string.as_str()] {
        let request = LocalAssetPolicy::request(raw, Some(&document)).unwrap_or_else(|| panic!("expected a local request for {raw}"));
        assert!(!request.is_safe_relative, "{raw}");
        assert!(!LocalAssetPolicy::allows(&request, None), "{raw}");
        let trusted = request.path();
        let authorizer: LocalAssetAuthorizer = Rc::new(move |path: &str| path == trusted);
        assert!(LocalAssetPolicy::allows(&request, Some(&authorizer)), "{raw}");
    }
}

#[test]
fn symlink_escape_is_blocked() {
    let directory = TemporaryDirectory::new();
    let outside = directory.0.with_extension("outside.png");
    std::fs::write(&outside, "not an image").unwrap();
    let link = directory.0.join("linked.png");
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    let document = file_url(directory.0.join("notes.md").to_str().unwrap());
    let request = LocalAssetPolicy::request("linked.png", Some(&document));
    let _ = std::fs::remove_file(&outside);
    let request = request.expect("a request");
    assert!(!request.is_safe_relative);
    assert!(!LocalAssetPolicy::allows(&request, None));
}

/// Not a Swift test: the NSURL composition matches Swift's `URL` on the
/// destinations the corpus and the probes exercise (see the module
/// documentation of `local_asset_policy`).
#[test]
fn destinations_resolve_like_swift_url() {
    let directory = TemporaryDirectory::new();
    let root = directory.0.to_str().unwrap().to_owned();
    let document = file_url(&format!("{root}/docs/notes.md"));
    let resolve = |raw: &str| {
        LocalAssetPolicy::request(raw, Some(&document)).map(|request| (url_path(&request.url), request.is_safe_relative))
    };
    // `URLByResolvingSymlinksInPath` spells the temporary directory the way
    // Swift does (`/var/…`, not `/private/var/…`).
    let docs = url_path(&LocalAssetPolicy::canonical_file_url(&file_url(&format!("{root}/docs"))).unwrap());
    let parent = url_path(&LocalAssetPolicy::canonical_file_url(&file_url(&root)).unwrap());
    // A `..` component anywhere is traversal, even when it stays inside.
    assert_eq!(resolve("a/./b/../c.png"), Some((format!("{docs}/a/c.png"), false)));
    assert_eq!(resolve("a/./c.png"), Some((format!("{docs}/a/c.png"), true)));
    assert_eq!(resolve("../up.png"), Some((format!("{parent}/up.png"), false)));
    assert_eq!(resolve("a/../../b.png"), Some((format!("{parent}/b.png"), false)));
    assert_eq!(
        resolve("img/q.png#gh-light-mode-only"),
        Some((format!("{docs}/img/q.png#gh-light-mode-only"), true))
    );
    assert_eq!(resolve("http://example.com/x.png"), None);
    assert_eq!(resolve(""), None);
    assert_eq!(resolve("/url"), Some(("/url".to_owned(), false)));
    assert!(LocalAssetPolicy::request("x.png", None).is_none());
}
