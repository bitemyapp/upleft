//! Port of `Tests/MarkdownRenderTests/ClipboardSemanticHTMLTests.swift`, the
//! `ClipboardSemanticHTML` cases. The paste-mode and inbound-payload cases
//! exercise `MarkdownSmartPaste` (View/), which the view branch ports.

use upleft_render::clipboard_semantic_html::ClipboardSemanticHTML;

#[test]
fn exports_common_markdown_as_semantic_escaped_html() {
    let html = ClipboardSemanticHTML::render(
        "# Read **this**

- one
- [two](https://example.com)

| Name | Value |
| --- | --- |
| Ada | `1` |

```swift
let x = 1 < 2
```",
    );
    assert!(html.contains("<h1>Read <strong>this</strong></h1>"), "{html}");
    assert!(html.contains("<ul><li>one</li><li><a href=\"https://example.com\">two</a></li></ul>"), "{html}");
    assert!(html.contains("<table><thead>"), "{html}");
    assert!(html.contains("<th>Name</th>"), "{html}");
    assert!(html.contains("<pre><code class=\"language-swift\">"), "{html}");
    assert!(html.contains("&lt;"), "{html}");
    assert!(!html.contains("<script"), "{html}");
}

#[test]
fn preserves_nested_mixed_lists_as_semantic_html() {
    let html = ClipboardSemanticHTML::render(
        "- parent
  1. ordered child
  2. second child
     - deep bullet
- sibling",
    );
    assert!(
        html.contains(
            "<ul><li>parent<ol><li>ordered child</li><li>second child<ul><li>deep bullet</li></ul></li></ol></li><li>sibling</li></ul>"
        ),
        "{html}"
    );
}

/// Beyond the Swift suite: the escaping and URL-scheme rules.
#[test]
fn raw_html_and_unsafe_links_stay_text() {
    let html = ClipboardSemanticHTML::render("<script>x</script> [a](javascript:alert(1)) ![i](img.png)\n");
    assert_eq!(
        html,
        "<p>&lt;script&gt;x&lt;/script&gt; <a href=\"\">a</a>) <img src=\"img.png\" alt=\"i\"></p>"
    );
    let html = ClipboardSemanticHTML::render("one  \ntwo ~~gone~~ __bold__\n");
    assert_eq!(html, "<p>one<br>\ntwo <del>gone</del> <strong>bold</strong></p>");
}
