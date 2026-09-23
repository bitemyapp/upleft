//! Port of `Tests/DownrightQLTests/QuickLookPolicyTests.swift`
//! ("Quick Look prefix limits"). All seven tests are ported;
//! `previewAppearanceContract` checks `PreviewAppearance` from
//! `upleft-render` (it needs AppKit's `NSAppearance`, not a window).

use std::path::PathBuf;

use objc2_app_kit::{NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua};
use objc2_foundation::NSArray;
use upleft_quicklook::preview_view_controller::PreviewViewController;
use upleft_quicklook::quick_look_loader::{QuickLookLoadedContent, QuickLookLoader};
use upleft_quicklook::quick_look_policy::QuickLookPolicy;
use upleft_render::theme::preview_appearance::PreviewAppearance;

fn temporary_directory() -> PathBuf {
    // The clock only ticks in microseconds here, so parallel tests need a
    // counter as well or two of them share (and delete) one directory.
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let unique = format!(
        "downright-ql-{}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    );
    let url = std::env::temp_dir().join(unique);
    std::fs::create_dir_all(&url).unwrap();
    url
}

struct Removing(PathBuf);

impl Drop for Removing {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `appearance.bestMatch(from: [.aqua, .darkAqua])`.
fn best_match(appearance: &NSAppearance) -> Option<String> {
    let names = NSArray::from_slice(&[unsafe { NSAppearanceNameAqua }, unsafe { NSAppearanceNameDarkAqua }]);
    appearance.bestMatchFromAppearancesWithNames(&names).map(|name| name.to_string())
}

#[test]
fn preview_appearance_contract() {
    assert_eq!(PreviewAppearance::System.title(), "System");
    assert!(PreviewAppearance::System.ns_appearance().is_none());
    assert_eq!(
        best_match(&PreviewAppearance::Light.ns_appearance().unwrap()),
        Some(unsafe { NSAppearanceNameAqua }.to_string())
    );
    assert_eq!(
        best_match(&PreviewAppearance::Dark.ns_appearance().unwrap()),
        Some(unsafe { NSAppearanceNameDarkAqua }.to_string())
    );
    assert_eq!(PreviewAppearance::ALL_CASES, [PreviewAppearance::System, PreviewAppearance::Light, PreviewAppearance::Dark]);
}

#[test]
fn memory_budget_is_incremental() {
    assert_eq!(QuickLookPolicy::MEMORY_CEILING_BYTES, 60 * 1024 * 1024);
    assert_eq!(PreviewViewController::preview_memory_bytes(94 * 1024 * 1024, 88 * 1024 * 1024), 6 * 1024 * 1024);
    assert_eq!(PreviewViewController::preview_memory_bytes(80, 88), 0);
}

#[test]
fn oversized_block_is_bounded() {
    let text = "x".repeat(8 * 1024 * 1024);
    let prefix = PreviewViewController::bounded_prefix(
        &text,
        QuickLookPolicy::PREFIX_RENDER_LIMIT_UTF16,
        QuickLookPolicy::PREFIX_RENDER_LIMIT_BYTES,
    );

    assert!(prefix.encode_utf16().count() as isize <= QuickLookPolicy::PREFIX_RENDER_LIMIT_UTF16);
    assert!(prefix.len() as isize <= QuickLookPolicy::PREFIX_RENDER_LIMIT_BYTES);
    assert!(upleft_swift_text::count(&prefix) < upleft_swift_text::count(&text));
}

#[test]
fn multibyte_character_remains_whole() {
    let text = "🌊".repeat(2_000);
    let prefix = PreviewViewController::bounded_prefix(&text, 101, 101);

    assert!(prefix.encode_utf16().count() <= 101);
    assert!(prefix.len() <= 101);
    assert_eq!(prefix.encode_utf16().count() % 2, 0);
    assert_eq!(prefix.len() % 4, 0);
}

/// `data.write(to: url, options: .atomic)`: a new inode renamed into place.
fn write_atomically(bytes: &[u8], url: &std::path::Path) {
    let staging = url.with_extension("staging");
    std::fs::write(&staging, bytes).unwrap();
    std::fs::rename(&staging, url).unwrap();
}

#[test]
fn growth_after_initial_stat_uses_prefix_path() {
    let directory = temporary_directory();
    let _cleanup = Removing(directory.clone());
    let url = directory.join("race.md");
    std::fs::write(&url, b"small\n").unwrap();
    let large = vec![0x61u8; QuickLookPolicy::FULL_READ_LIMIT_BYTES as usize + 1_024];

    let before_read = || write_atomically(&large, &url);
    let result = QuickLookLoader::load(&url, 6, Some(&before_read));

    let Some(QuickLookLoadedContent::Prefix(text)) = result else {
        panic!("A file that grows after stat must take the bounded prefix path");
    };
    assert!(text.len() as isize <= QuickLookPolicy::FULL_READ_LIMIT_BYTES);
}

#[test]
fn bounded_loader_preserves_utf16_and_utf32() {
    let directory = temporary_directory();
    let _cleanup = Removing(directory.clone());
    let url = directory.join("encoded.md");

    let mut utf16 = vec![0xFF, 0xFE];
    utf16.extend("# UTF-16\n".encode_utf16().flat_map(u16::to_le_bytes));
    std::fs::write(&url, &utf16).unwrap();
    assert_eq!(
        QuickLookLoader::load(&url, utf16.len() as isize, None),
        Some(QuickLookLoadedContent::Full("# UTF-16\n".into()))
    );

    let mut utf32 = vec![0x00, 0x00, 0xFE, 0xFF];
    utf32.extend("# UTF-32\n".chars().flat_map(|c| (c as u32).to_be_bytes()));
    std::fs::write(&url, &utf32).unwrap();
    assert_eq!(
        QuickLookLoader::load(&url, utf32.len() as isize, None),
        Some(QuickLookLoadedContent::Full("# UTF-32\n".into()))
    );
}

#[test]
fn bounded_loader_normalizes_crlf() {
    let directory = temporary_directory();
    let _cleanup = Removing(directory.clone());
    let url = directory.join("crlf.md");
    let data = b"# Heading\r\n\r\nBody\r\n";
    std::fs::write(&url, data).unwrap();

    assert_eq!(
        QuickLookLoader::load(&url, data.len() as isize, None),
        Some(QuickLookLoadedContent::Full("# Heading\n\nBody\n".into()))
    );
}

#[test]
fn replacement_during_large_preview_remains_bounded() {
    let directory = temporary_directory();
    let _cleanup = Removing(directory.clone());
    let url = directory.join("large.md");
    std::fs::write(&url, b"old\n").unwrap();
    let replacement = vec![0x61u8; QuickLookPolicy::PREFIX_READ_LIMIT_BYTES as usize + 5_000];

    let before_read = || write_atomically(&replacement, &url);
    let result = QuickLookLoader::load(&url, QuickLookPolicy::LARGE_FILE_THRESHOLD_BYTES + 1, Some(&before_read));

    let Some(QuickLookLoadedContent::Prefix(text)) = result else {
        panic!("Large-file previews must remain prefix loads");
    };
    assert!(text.len() as isize <= QuickLookPolicy::PREFIX_READ_LIMIT_BYTES);
}

/// Not in the Swift suite: `FileHandle.read(upToCount:)` is `nil` at end of
/// file (probed on Swift 6.4), so an empty file gives no preview at all.
#[test]
fn empty_file_gives_no_preview() {
    let directory = temporary_directory();
    let _cleanup = Removing(directory.clone());
    let url = directory.join("empty.md");
    std::fs::write(&url, b"").unwrap();
    assert_eq!(QuickLookLoader::load(&url, 0, None), None);
}
