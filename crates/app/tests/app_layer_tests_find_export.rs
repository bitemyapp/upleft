//! The find (§9.4), export (§9.5) and slug cases of
//! `Tests/DownrightAppTests/AppLayerTests.swift`. The rest of that file
//! belongs to the other app-layer ports.

use std::path::PathBuf;

use upleft_app::export::html_exporter::{HTMLExporter, Slugs};
use upleft_app::support::find_engine::{FindEngine, FindQuery, FindSession};
use upleft_core::parser::MarkdownParser;
use upleft_foundation::url::FileUrl;
use upleft_render::theme::theme_store::ThemeStore;

fn count(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

// MARK: - Find (§9.4)

#[test]
fn find_literal_regex_and_whole_word() {
    let text = "alpha beta alphabet ALPHA";

    let mut query = FindQuery::new("alpha");
    assert_eq!(FindEngine::matches(text, &query).len(), 3, "case-insensitive by default");

    query.case_sensitive = true;
    assert_eq!(FindEngine::matches(text, &query).len(), 2);

    query.whole_word = true;
    assert_eq!(FindEngine::matches(text, &query).len(), 1, "alphabet must not match");

    let regex = FindQuery::regex("al(pha|beit)");
    assert_eq!(FindEngine::matches(text, &regex).len(), 3);
}

#[test]
fn find_treats_special_characters_literally_when_not_regex() {
    let mut query = FindQuery::new("a.c");
    assert_eq!(FindEngine::matches("abc a.c", &query).len(), 1);
    query.is_regex = true;
    assert_eq!(FindEngine::matches("abc a.c", &query).len(), 2);
}

#[test]
fn half_typed_regex_returns_nothing_rather_than_throwing() {
    let query = FindQuery::regex("a(");
    assert!(FindEngine::matches("aaa", &query).is_empty());
    assert!(!FindEngine::is_valid(&query));
}

#[test]
fn regex_replacement_expands_capture_groups() {
    let query = FindQuery::regex(r"(\w+)@(\w+)");
    let text = "user@host";
    let found = *FindEngine::matches(text, &query).first().expect("a match");
    assert_eq!(FindEngine::replacement(found, text, &query, "$2/$1"), "host/user");
}

#[test]
fn find_session_advances_and_wraps() {
    let mut session = FindSession::new();
    session.update(FindQuery::new("x"), "x--x--x", 0);
    assert_eq!(session.count(), 3);
    assert_eq!(session.status_text(), "1 of 3");
    session.advance(true);
    assert_eq!(session.status_text(), "2 of 3");
    session.advance(true);
    session.advance(true);
    assert_eq!(session.status_text(), "1 of 3", "wraps");
}

// MARK: - Export (§9.5)

fn exporter(markdown: &str, base_directory: Option<FileUrl>) -> HTMLExporter {
    HTMLExporter::new(MarkdownParser::parse(markdown), ThemeStore::shared().current(), "T", base_directory, None)
}

#[test]
fn html_export_is_self_contained_and_escaped() {
    let markdown = "# Title & <Tag>\n\nA paragraph with **bold**, `code`, and a [link](https://example.com).\n\n\
                    - [x] done\n- [ ] not done\n\n| a | b |\n|---|--:|\n| 1 | 2 |\n\n```swift\nlet x = \"<script>\"\n```";
    let html = HTMLExporter::new(MarkdownParser::parse(markdown), ThemeStore::shared().current(), "Test", None, None).html();

    assert!(html.contains("<style>"), "styles must be inlined");
    assert!(!html.contains("<link rel=\"stylesheet\""), "must not reference an external stylesheet");
    assert!(!html.contains("<script src="), "must not reference external scripts");
    assert!(html.contains("Title &amp; &lt;Tag&gt;"), "heading text must be escaped");
    assert!(html.contains("&lt;script&gt;"), "code contents must be escaped");
    assert!(html.contains("<strong>"));
    assert!(html.contains("type=\"checkbox\""));
    assert!(html.contains("<table>"));
    assert!(html.contains("text-align:right"), "table alignment must survive");
}

#[test]
fn html_export_rewrites_relative_markdown_links() {
    let html = exporter("See [the plan](plan.md) and [the web](https://example.com).", None).html();
    assert!(html.contains("href=\"plan.html\""), "sibling exports stay navigable");
    assert!(html.contains("href=\"https://example.com\""), "absolute links are untouched");
}

#[test]
fn html_export_preserves_sibling_link_queries_and_anchors() {
    let html = exporter(
        "[section](plan.md#steps) [query](plan.md?mode=read#steps)\n\
         [encoded](my%20plan.md#next) [remote](https://example.com/plan.md#steps)\n\
         [network](//example.com/plan.md) [mail](mailto:plan.md)\n\
         [spaces](<my plan.md#steps>)",
        None,
    )
    .html();
    for destination in [
        "plan.html#steps",
        "plan.html?mode=read#steps",
        "my%20plan.html#next",
        "https://example.com/plan.md#steps",
        "//example.com/plan.md",
        "mailto:plan.md",
        "my plan.html#steps",
    ] {
        assert!(html.contains(&format!("href=\"{destination}\"")), "{destination}");
    }
}

#[test]
fn html_export_without_an_asset_root_keeps_images_inert() {
    let html = exporter("![relative](photo.png) ![absolute](/tmp/photo.png) ![parent](../photo.png)", None).html();
    assert!(!html.contains("<img"));
    assert_eq!(count(&html, "class=\"missing\""), 3);
    assert!(html.contains("photo.png"));
}

/// Regression: wikilink targets used to ship as live `href`s with no scheme
/// check.
#[test]
fn html_export_makes_unsafe_wikilink_targets_inert() {
    let html = exporter(
        "[[javascript:alert(document.domain)//]] [[data:text/html;base64,x]]\n[[Notes]] [[deep/note|with a label]]",
        None,
    )
    .html();
    assert!(!html.to_lowercase().contains("href=\"javascript:"), "script wikilinks must not be live");
    assert!(!html.to_lowercase().contains("href=\"data:"), "data wikilinks must not be live");
    assert!(!html.contains("href=\"javascript:alert(document.domain)//.html\""));
    assert!(html.contains("href=\"Notes.html\""), "plain wikilinks still export");
    assert!(html.contains("href=\"deep/note.html\""), "labelled wikilinks still export");
    assert!(html.contains(">with a label<"));
}

#[test]
fn html_export_keeps_deep_heading_hierarchy() {
    let html = exporter("# H1\n\n## H2\n\n### H3\n\n#### H4\n\n##### H5\n\n###### H6\n", None).html();
    assert!(html.contains("h1 { color:") && html.contains("font-weight: 700; letter-spacing: -0.022em"));
    assert!(html.contains("h2 { color:") && html.contains("font-weight: 700; letter-spacing: -0.014em"));
    assert!(html.contains("h3 { color:") && html.contains("font-weight: 700; letter-spacing: -0.014em"));
    assert!(html.contains("font-weight: 600; letter-spacing: normal"));
    assert!(html.contains("font-weight: 600; letter-spacing: 0.04em"));
    assert!(html.contains("font-weight: 500; font-style: italic; letter-spacing: 0.06em"));
}

// MARK: - HTML export confinement (§9.5)

fn temporary(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("{name}-{}", upleft_core::Uuid::new_v4().hyphenated().to_string().to_uppercase()))
}

struct Removing(PathBuf);

impl Drop for Removing {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
        let _ = std::fs::remove_file(&self.0);
    }
}

fn make_export_root() -> (FileUrl, Removing) {
    let root = temporary("downright-export");
    std::fs::create_dir_all(&root).unwrap();
    (FileUrl::from_path_is_directory(&root.to_string_lossy(), true), Removing(root))
}

/// A 1×1 transparent PNG, small enough that embedding succeeds.
fn png_data() -> Vec<u8> {
    const BASE64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
    let mut out = Vec::new();
    let mut buffer = 0u32;
    let mut bits = 0;
    for byte in BASE64.bytes().filter(|&b| b != b'=') {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            _ => 63,
        } as u32;
        buffer = buffer << 6 | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
        }
    }
    out
}

fn export_html(markdown: &str, base_directory: &FileUrl) -> String {
    exporter(markdown, Some(base_directory.clone())).html()
}

#[test]
fn html_export_embeds_only_images_inside_the_base_directory() {
    let (root, _cleanup) = make_export_root();
    std::fs::write(root.appending_path_component("inside.png").path(), png_data()).unwrap();
    // A sibling file outside the export root: the leak that motivated the
    // containment rule.
    let outside = temporary("downright-outside").with_extension("png");
    std::fs::write(&outside, png_data()).unwrap();
    let _outside = Removing(outside.clone());

    let name = outside.file_name().unwrap().to_string_lossy();
    let html = export_html(&format!("![in](inside.png) ![leak](../{name}) ![abs](/etc/hosts)"), &root);

    assert!(html.contains("data:image/png;base64,"));
    assert!(html.contains("data:image/png;base64,iVBORw0KGgo"));
    assert!(html.chars().filter(|&c| c == ',').count() >= 1);
    assert_eq!(count(&html, "data:image/"), 1, "escapes must not be base64-embedded");
    assert!(html.contains("class=\"missing\""));
    assert_eq!(count(&html, "class=\"missing\""), 2);
}

#[test]
fn html_export_refuses_traversal_and_absolute_paths() {
    let (root, _cleanup) = make_export_root();
    let secret = temporary("downright-secret").with_extension("png");
    std::fs::write(&secret, png_data()).unwrap();
    let _secret = Removing(secret.clone());

    // `../` points at the temporary directory the secret lives in.
    let parent = root.deleting_last_path_component();
    let traversal = format!("../{}/{}", parent.last_path_component(), secret.file_name().unwrap().to_string_lossy());
    let html = export_html(&format!("![t]({traversal}) ![a](/private/var/etc/passwd)"), &root);

    assert!(!html.contains("data:image/"), "no local file may be embedded");
    assert_eq!(count(&html, "class=\"missing\""), 2);
}

#[test]
fn html_export_does_not_embed_oversized_images() {
    let (root, _cleanup) = make_export_root();
    std::fs::write(root.appending_path_component("huge.png").path(), vec![0xFF; 5 * 1024 * 1024 + 1]).unwrap();

    let html = export_html("![big](huge.png)", &root);
    assert!(!html.contains("data:image/"));
    assert!(html.contains("class=\"missing\""));
}

#[test]
fn html_export_never_retains_active_external_image_sources() {
    let (root, _cleanup) = make_export_root();
    let html = export_html(
        "![web](https://tracker.example/pixel.png) ![file](file:///private/etc/passwd) \
         ![data](data:image/svg+xml,<svg/>) ![protocol](//tracker.example/pixel.png)",
        &root,
    );

    assert!(!html.contains("<img src=\"https://"));
    assert!(!html.contains("<img src=\"file:"));
    assert!(!html.contains("<img src=\"data:"));
    assert!(!html.contains("<img src=\"//"));
    assert_eq!(count(&html, "class=\"missing\""), 4);
}

#[test]
fn slugs_match_git_hub_conventions() {
    assert_eq!(Slugs::make("Hello, World!"), "hello-world");
    assert_eq!(Slugs::make("  spaced  out  "), "spaced-out");
    assert_eq!(Slugs::make("§8.1 Rendered diff"), "81-rendered-diff");
}
