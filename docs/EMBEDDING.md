# Embedding the renderer

`upleft-render` can draw Markdown inside another application's views. Downright's document surface (`MarkdownContainerView` with its scroller, gutter and footnote margin) is one way to use it. **Hosted mode** is the other: each `MarkdownTextView` is one message in a host's own scroll view, sized to its content. A chat transcript is the case it is built for, and Omperor is the first host.

Hosted mode is an Upleft extension with no Swift counterpart. It is off unless a view is made with `MarkdownTextView::new_hosted`, and nothing in Downright's document path changes.

## At start-up

Call these once, on the main thread, before the first view is made.

- **Mermaid.** `upleft_mermaid::downright::mermaid_renderer_bridge::install_fragment_renderer()`. The render crate can't depend on the Mermaid crate, so the host connects them. This installs both the synchronous renderer that Downright's path uses and the thread-safe one that hosted views use. Without it, every diagram draws as a failed object.
- **Math fonts.** Ship `mathFonts.bundle` in the app's `Contents/Resources`. The resolver looks there, beside the executable, in `$UPLEFT_MATH_FONTS`, and in the submodule copy in development builds. A host that keeps the bundle anywhere else calls `upleft_math::math_bundle::math_resource_bundle::set_math_fonts_directory(path)` before the first formula. The fonts load on first use. In a hosted view, first use happens on a worker.
- **Themes.** `ThemeStore::bundled_themes()` returns the shipped themes without reading preferences or touching the disk. `Theme::fallback()` is the built-in default. Do not call `ThemeStore::shared()` unless you want Downright's user themes folder and its watcher.

## Minimal code

```rust
use std::rc::{Rc, Weak};
use upleft_core::DirtySet;
use upleft_core::parser::MarkdownParser;
use upleft_render::render_contracts::Theme;
use upleft_render::theme::style_sheet::{HostTypography, StyleSheet};
use upleft_render::view::markdown_text_view::MarkdownTextView;
use upleft_render::view::markdown_text_view_delegate::MarkdownTextViewDelegate;

struct Row;
impl MarkdownTextViewDelegate for Row {
    fn did_change_content_height(&self, view: &MarkdownTextView, height: f64) {
        // Restack the transcript now; this runs inside `update`.
    }
    fn did_activate_link(&self, view: &MarkdownTextView, destination: &str, range: NSRange, modifiers: NSEventModifierFlags) {
        // Open it, or don't. The view never opens a link itself.
    }
}

let sheet = Rc::new(StyleSheet::for_host(Theme::fallback(), &appearance, reduce_motion, HostTypography::default()));
let storage: Retained<NSTextStorage> =
    unsafe { msg_send![NSTextStorage::alloc(), initWithString: &*NSString::from_str(text)] };
let view = MarkdownTextView::new_hosted(&storage, sheet, column_width, mtm);
let delegate: Rc<dyn MarkdownTextViewDelegate> = Rc::new(Row);   // keep it alive
view.set_markdown_delegate(Some(Rc::downgrade(&delegate)));
stack.addSubview(&view);
view.update(MarkdownParser::parse(text), &DirtySet::wholesale(), false);
// view.frame().size is now the message's size; place it with setFrameOrigin.
```

`crates/render/examples/hosted_transcript.rs` is a complete host: a transcript window, streaming, restacking and a scroll that follows the bottom.

## The hosted API

On `MarkdownTextView`:

| Call | What it does |
|---|---|
| `new_hosted(storage, style_sheet, width, mtm)` | Makes a hosted view in Read mode. `width` is the text container width. |
| `set_hosted_width(width)` / `hosted_width()` | Sets the text container width directly: there is no responsive measure, gutter or margin. The view lays out again and reports its height before returning. |
| `set_hosted_insets(NSSize)` | The view's own margins around the text (its `textContainerInset`). The default is zero. The frame is the width plus twice the horizontal inset, by the content height plus twice the vertical inset. |
| `update(document, dirty, preserving_selection)` | As for Downright (below). |
| `content_height()` | The laid-out height plus the vertical insets. It is exact and comes from TextKit's layout. |
| `set_streaming(bool)` / `is_streaming()` | See "Streaming". |
| `set_style_sheet(Rc<StyleSheet>)` | Restyles the view; call it when the appearance or the typography changes. |
| `prepare_for_display()` | Lays out and draws the visible part now, as Downright's first frame does. It is optional: the window's own display cycle does the same. |
| `is_hosted()` | Whether the view was made by `new_hosted`. |
| `hosted_line_at(y)` / `hosted_line_top(line)` | The line at `y` in view coordinates, as a `HostedLine` (its paragraph's source and TextKit offsets, and where the line starts in the paragraph) plus its top; and where that line is now. A host keeps the reader's place by the line at its viewport's top: after the text above it, a diagram landing, or a new width, `hosted_line_top` finds the line again (the line that now holds the same character). Both read the per-fragment heights (below), not TextKit's fragment origins. |
| `hosted_pending_objects()` | The diagrams and formulas the view draws as placeholders while they render, as their fragments' tops and bottoms in view coordinates. |
| `set_math_copy_as_tex(bool)` / `math_copy_as_tex()` | Copy, and a drag of the selection, write each formula the selection touches as its TeX: `$…$` inline, `$$…$$` with its delimiters on lines of their own for display math and `math` fences. A selection that starts or ends inside a formula takes it whole. Plain text and RTF carry the TeX in place of the image, HTML carries it as text, and the private Markdown flavour stays the source. The mapping runs when the selection is written to a pasteboard, from the parsed document, and nowhere else: rendering and scrolling are unchanged. Off by default, which is Downright's copy. |
| `math_latex_at_source_offset(offset)` | The LaTeX, without delimiters, of the formula at a source offset (a `ContextTarget`'s `hit_offset`), for a host's Copy LaTeX item. |

The view sets its own frame size and never its origin. The host positions it. `setDrawsBackground(false)` shows the host's background through the view; `setBackgroundColor` is reset from the style sheet when the style sheet changes.

On the delegate (`MarkdownTextViewDelegate`, every method optional):

- `did_change_content_height(view, height)` is called **synchronously** whenever the height changes: from `update`, `set_hosted_width`, `set_hosted_insets`, `set_style_sheet`, a code block's collapse chip, and the relayout after a diagram, formula or image lands. The frame already has the new height, so the host can restack its rows in the same run-loop pass.
- `did_activate_link` receives every link click. A hosted view overrides `clickedOnLink:atIndex:`, so AppKit never opens the link.
- `did_toggle_checkbox_at_mark_offset` receives a task checkbox click. The view does not edit the text or animate the box. To accept the toggle, the host edits the storage and calls `update`. Otherwise nothing changes on screen.
- `did_navigate_to` receives a footnote reference click. The view does not scroll.
- `did_activate_image`, `did_activate_path_token`, `wants_context_menu_for` and the rest work as in Downright.

`upleft_render::fragments::async_objects::pending_count()` is the number of diagrams and formulas rendering on workers. Tests use it to wait for a view to settle.

## What hosted mode changes

- **Height.** A hosted view is exactly as tall as its content plus its insets. There is no overscroll, no minimum of the viewport height, and no deferred line-count or semantic resize. After an edit, only the fragments from the paragraph before the edit onwards are laid out again. A stream appends, so that is the last few paragraphs. The height is the sum of the fragments' heights, which the view keeps per fragment. TextKit 2's usage bounds are not exact here: after a fragment changes height, the fragments below it keep their old origins until viewport layout reaches them. Drawing is right, but bounds built from those origins are not.
- **No scrolling.** The view never scrolls, restores or observes the enclosing scroll view, which belongs to the host. `scroll_to_offset` and `scrollRangeToVisible:` do nothing, `update` captures no viewport anchor, and nothing restores one. TextKit's viewport layout still works when the view is taller than the visible area or only partly visible. It lays out what is visible and draws correctly as the host scrolls; `hosted_transcript` checks this pixel for pixel. The one AppKit scroll left is the autoscroll while the reader drags a selection past the edge. The host's scroll view performs it, as it does for any view inside it.
- **Read mode.** The view is selectable and not editable. A task checkbox never changes on its own (above).
- **Diagrams and display math** render off the main thread (below).
- **Decoration** does not use the engine's program cache. With the cache, a block's attributes depend on how often the block was decorated before. Replaying a cached program restyles only the block's own range, while the live path also restyles the physical paragraph around it, such as a list item's marker. A streamed message must end up as if it had been decorated once.

## Streaming

A host appends to the last message about 30 times a second. For each append:

1. Off the main thread, parse the whole new text and diff it against the previous parse. `MarkdownParser::parse(&text)` returns an `Arc<ParsedDocument>`, and `ASTDiff::dirty_set(Some(&previous), &fresh)` returns a `DirtySet`. Both are plain Rust and `Send`.
2. On the main thread, **in the same turn**, edit the storage and call `update(fresh, &dirty, true)`:

   ```rust
   storage.beginEditing();
   storage.replaceCharactersInRange_withString(NSRange::new(storage.length(), 0), &NSString::from_str(&piece));
   storage.endEditing();
   view.update(fresh, &dirty, true);
   ```

   The storage and the parse must describe the same text when `update` runs. If a display cycle runs between the edit and the update, the edited paragraphs draw as plain text for that frame.
3. Before the first append, call `view.set_streaming(true)`. After the last one, call `set_streaming(false)`.

While streaming, a Mermaid or math fence the stream has not closed yet (no closing fence, at the end of the message) is decorated as a plain code block. Appending to it relays out one code block and never renders a diagram. When the closing fence arrives, the block becomes a diagram or formula and renders on a worker. `set_streaming(false)` renders a fence that was never closed as what it says it is.

A hosted `update` keeps its whole-document passes proportional to the edit: the paragraph index, the base display map, the elided-attribute pass, the accessibility children and the layout. It also decorates everything a whole-text update would have changed, so a streamed message ends up identical to the same message given whole. That includes the inserted text itself, whole physical paragraphs, and blocks whose parse changed because of later text: a footnote or link reference defined further down, or safe HTML paired with a closing tag in a later block. The diff cannot see those blocks, because it compares each block's own bytes. `hosted_embedding_tests` checks storage attributes, display maps, paragraphs and heights for streamed and whole messages, and `hosted_transcript` checks pixels.

Appends are cheap. On an Apple M5 Max, in a release build, an append to a 200 KB message costs 2.3 ms at p50 and 3.3 ms at p99. That covers `update` with the storage edit and the host's restack, `prepare_for_display`, and the window's draw. The worst append, 6.5 ms, was the one that completed a footnote definition, which re-decorates every paragraph citing it. On Downright's document path the same appends cost 29 ms at p50. The first update of a whole message lays all of it out, because the height must be exact: about 390 ms for 200 KB. A host restoring a long history should create the visible rows first.

## One document as several views

A host may show a long document as several hosted views stacked one above the other, cut before top-level blocks, so that only the views in sight are laid out. Each view must then present its part exactly as one view of the whole document would. Two things make that possible:

- **`MarkdownParser::parse_segment(text, &SegmentContext)`** parses a part as it parses inside the whole document. The context carries what the rest of the document contributes, and none of it is in the part's text: the link reference and footnote definitions the part cites (cmark resolves against them as against definitions placed before the part), the labels the rest defines again after the part (a document keeps the last definition of a label, so the part's own is not hidden in Read mode), and whether a `<details>` opening tag comes before the part or a closing tag after it, which a lone tag in the part pairs with. `SafeHTMLParser::details_tags` finds those tags in a part's text as pairing does. The context is kept in `ParsedDocument::segment_context`, and `update` re-decorates the blocks whose parse a changed context changed, as it does for a definition added later in the same view.
- **`set_hosted_continuation(true)`**, before the first `update`, marks a view whose part comes after a heading of the document: its first heading is not the document's first, and keeps the space above it.
- **`set_hosted_continued(true)`** marks a view that another view continues below. TextKit lays out an empty line after a text's final line break; the view's height leaves it out, so the view ends where the whole document would go on with the next part.

TextKit drops the space before a view's first paragraph, which the whole document would show between the parts; the host adds it above the view. It does so exactly for lines of text, not for what a fragment draws from its frame (a code or callout band, a quote bar, a table, a centred diagram, formula, image or rule), so a part should open with prose after a blank line. Omperor checks every cut against the document parsed whole, and passes over a boundary where anything would differ.

### Pixels

TextKit draws each layout fragment of a view in a view of its own (`_NSTextViewportElementView`), which it sets on the window's device pixels when it lays out the viewport, rounding out from the fragment's exact place, and moves what it has drawn without drawing it again when the text view moves. Two renderings of the same lines are therefore pixel-identical only if their fragments were laid out with the text at the same place on the pixel grid. For a host that stacks views:

- **`set_hosted_content_offset(offset)`** starts the text `offset` points below the view's top (the view is that much taller). Put the view's frame on a whole device pixel and the text's remaining fraction in the offset: the view's pixels then depend only on where its text is. A new offset lays the viewport out again and draws every fragment again.
- **`redraw_surfaces()`** lays the viewport out and draws every fragment again where it now is. Call it after moving a hosted view by part of a device pixel, if you do not keep its frame on whole pixels.
- Scroll by whole device pixels, and keep the document's height on them: a scroll by part of a pixel leaves every fragment drawn before it off the grid.
- A hosted update that changes a fragment's height draws the fragments after it again when it moves them by part of a pixel.

## Diagrams and math off the main thread

In a hosted view, a Mermaid or display-math fragment looks its image up in the same shared caches Downright's path fills. On a miss, it schedules the render on a global queue, once per key however many fragments ask. Until then it draws a placeholder: a card in the code background colour, sized to what the key rendered at before or to an estimate from the source. When the render lands, the image goes into the shared cache, and every hosted view that asked lays out the blocks with that source again and reports its new height. A key that is still rendering stays a placeholder even if the worker has already filled the cache, so a fragment's height only changes when the landing lays it out. A render that fails is remembered, and the fragment draws Downright's failure card.

Everything on the worker was checked for thread safety. `hosted_transcript` renders each diagram and formula of the stress document on the main thread and on a worker, and the bitmaps are identical.

- **Mermaid.** Parsing, ELK layout, and drawing into a private `CGBitmapContext`. Text is drawn with Core Text and with `NSAttributedString` drawing into an `NSGraphicsContext` the worker makes for that bitmap. AppKit's threading guide allows this ("generally thread-safe when drawing with its graphics functions and classes, including NSBezierPath and NSString"). Per-thread state in `upleft-mermaid` and `upleft-elk` is thread-local. Two calls in Downright's path are not safe off the main thread, and the worker path avoids both:
  - `NSScreen.mainScreen` supplied the backing scale. The scale is now read on the main thread and passed in (`mermaid_renderer_bridge::image_at_scale`).
  - `NSFontManager.sharedFontManager` converted fonts to italic. Off the main thread the italic comes from `NSFontDescriptor` symbolic traits instead. `hosted_transcript` checks that both give the same font.
- **Display math.** Parsing and typesetting are Core Text and plain Rust, and the font tables sit behind locks. The formula becomes an `NSImage` with a drawing handler, padded by drawing it into a second `NSImage` with `lockFocus`. AppKit's guide allows this: "one thread can create an NSImage object, draw to the image buffer, and pass it off to the main thread for drawing."
- **The style sheet** travels to the worker as a clone. Its colours were snapshotted to sRGB against one appearance when it was built, so nothing on the worker resolves a dynamic colour against the wrong appearance.

Inline math still typesets on the main thread when its paragraph is decorated. The math cache makes that cheap, and inline math is part of the text layout.

## Threading rules

- Views, style sheets, storages and delegates are main-thread objects.
- Parse and diff on any thread. Edit the storage and call `update` on the main thread, in the same turn.
- `StyleSheet::for_host` is cheap but touches fonts and colours: build it on the main thread.
- Never lay out or draw a view from another thread. The only work the view sends to other threads is the diagram and formula rendering above, and the results come back through the main queue.

## Typography

`StyleSheet::for_host(theme, appearance, reduce_motion, typography)` builds a style sheet without `ThemeStore`. It reads and writes no preferences, and it creates and watches no directory. Its `revision` is 0. Increase Contrast and Reduce Transparency come from `NSWorkspace`, as in Downright. When the appearance changes, build a new one and call `set_style_sheet` on each view.

`HostTypography` adds host-only fields. Every field is optional, and `None` keeps Downright's behaviour. Theme JSON is untouched, and a hosted style sheet with `HostTypography::default()` resolves the same fonts and metrics as `StyleSheet::new`.

| Field | Effect |
|---|---|
| `body_family: Option<BodyFamily>` | `System` (SF Pro), `NewYork`, `Monospaced` (SF Mono, the monospaced system font), or `Named("Charter")`, which falls back to the system font if the family is not installed. Headings use the same family. `None` follows the theme's preset. |
| `body_size: Option<f64>` | Body size in points. The heading scale, code size, math size and indents follow it. |
| `heading_sizes: [Option<f64>; 6]` | H1–H6 sizes in points, in place of the modular scale. |
| `code_size: Option<f64>` | Code size in points, in place of `bodySize × monoSizeAdjust` normalised to SF Mono's x-height. |
| `line_height_multiple: Option<f64>` | Line height as a multiple of the body size, kept to the half point rather than rounded to Downright's baseline grid. The grid becomes a quarter of it, so heading spacing and other grid multiples keep their proportions. |
| `paragraph_spacing: Option<f64>` | Space after a top-level paragraph, in points. List paragraphs keep their tighter spacing. |
| `hyphenation_factor: Option<f32>` | `NSParagraphStyle.hyphenationFactor` for paragraphs, list items, quotes, callouts and footnotes. |
| `code_bleed: Option<f64>` | The lane that code, tables, math and diagrams may use past the prose's trailing edge (88 pt in Downright). A chat column usually wants 0, so prose is as wide as code. |

Omperor's three chat styles, as `hosted_transcript` writes them:

```rust
let headings = [Some(23.0), Some(20.0), Some(17.0), Some(15.0), None, None];
let modern = HostTypography { body_family: Some(BodyFamily::System), body_size: Some(15.0), heading_sizes: headings,
    line_height_multiple: Some(1.5), paragraph_spacing: Some(11.0), code_bleed: Some(0.0), ..Default::default() };
let editorial = HostTypography { body_family: Some(BodyFamily::NewYork), body_size: Some(15.5), heading_sizes: headings,
    line_height_multiple: Some(1.55), paragraph_spacing: Some(13.0), hyphenation_factor: Some(0.9), code_bleed: Some(0.0),
    ..Default::default() };
let terminal = HostTypography { body_family: Some(BodyFamily::Monospaced), body_size: Some(14.0),
    heading_sizes: [Some(17.0), Some(16.0), Some(15.0), Some(14.0), None, None], code_size: Some(13.0),
    line_height_multiple: Some(1.45), paragraph_spacing: Some(9.0), code_bleed: Some(0.0), ..Default::default() };
```

Per-role size overrides, such as a smaller size for user turns, are separate style sheets with different `body_size` and `heading_sizes`.

## Objective-C classes

The crate registers these classes in the process (objc2 `define_class!`, registered on first use). A host must not register classes with the same names:

`MarkdownTextView`, `MarkdownContainerView`, `MarkdownContentStorage`, `MarkdownTextParagraph`, `ParagraphSubstitution`, `FragmentProvider`, `FragmentPayload`, `FragmentAccessibilityElement`, `DownrightFragment`, `ProseFragment`, `ElidedFragment`, `ElisionCueFragment`, `GutterRailView`, `HeadingMenuAction`, `FootnoteMarginView`, `SpringSurfaceView`, `DensityGutterView`, `DensityGutterPreviewWindow`, `PreviewContentView`, `DensityOutlineWindow`, `OutlineTableView`, `DensityOutlineRow`, `OutlineBackdrop`, `UpleftBlockIdentityValue`, `UpleftPathTokenValue`, `UpleftSpringDriverTarget`.

Subclasses of `DownrightFragment` are also registered at run time, under the names Downright's layout dumps use: `CodeBlockFragment`, `TableRowFragment`, `CalloutFragment`, `ImageFragment`, `ListOrnamentFragment`, `FrontMatterFragment`, `MathFragment`, `MermaidFragment`, `ThematicBreakFragment`.

`upleft-math`, `upleft-mermaid`, `upleft-elk`, `upleft-core` and `upleft-markup` register no classes.

## Limitations

- The first update of a whole message lays out all of it, on the main thread (about 390 ms for 200 KB). The height must be exact, and TextKit lays out only on the main thread.
- While a diagram or formula renders, its block is a placeholder of estimated height. The message's height changes once when the image lands. A key the view has rendered before takes that size at once.
- Hyphenation applies to whole paragraphs. TextKit has no per-run switch, so inline code in a hyphenated paragraph can break at a hyphen.
- A hosted view reads the main screen's backing scale when it schedules a diagram, as Downright does when it draws one. A window that moves to a screen with another scale keeps the diagrams it has until they are laid out again.
- Hosted views are built and tested for Read mode. `set_mode` still works on them, but editing in a hosted view (Live or Source mode) is untested.
- Source focus (`focus_source`) works in a hosted view but takes the full base-display-map pass on every update.
- A hosted view still captures a viewport anchor in `set_mode`, `set_configuration` and `set_style_sheet`. Nothing uses it, and nothing scrolls.
