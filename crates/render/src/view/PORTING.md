# The view layer: porting notes

The port of `Sources/MarkdownRender/View`, `Fragments/FragmentBase.swift`,
`Fragments/InlineCodePill.swift` and `Motion.swift`.

## File map

| Swift | Rust |
|---|---|
| `View/MarkdownTextView.swift` | `view/markdown_text_view.rs` (the class, its state and every Objective-C override) |
| `View/MarkdownTextView+Interaction.swift` | `view/markdown_text_view_interaction.rs` (the methods; the overrides are registered in `markdown_text_view.rs`) |
| `View/MarkdownContentStorage.swift` | `view/markdown_content_storage.rs` |
| `View/ParagraphSubstitution.swift` | `view/paragraph_substitution.rs` |
| `View/FragmentProvider.swift` | `view/fragment_provider.rs` |
| `View/MarkdownContainerView.swift` | `view/markdown_container_view.rs` |
| `View/GutterRailView.swift` | `view/gutter_rail_view.rs` |
| `View/FootnoteMarginView.swift` | `view/footnote_margin_view.rs` |
| `View/TrackingArea.swift` | `view/tracking_area.rs` |
| `View/DensityGutterView.swift` | `view/density_gutter_view.rs` (`DensityGutterView`, `DensityBand`, `MarkSimulation`, `PipSimulation`, the stack model) |
| `View/DensityGutterPreviewWindow.swift` | `view/density_gutter_preview_window.rs` (`DensityGutterPreviewWindow`, `PreviewContentView`) |
| `View/DensityOutlineWindow.swift` | `view/density_outline_window.rs` (`DensityOutlineEntry`, `DensityOutlineWindow`, `OutlineTableView`, `DensityOutlineRow`, `OutlineBackdrop`) |
| `View/MarkdownTextViewDelegate.swift` | `view/markdown_text_view_delegate.rs` |
| `View/MarkdownSmartPaste.swift` | `view/markdown_smart_paste.rs` |
| `MarkdownTextView.rebuildBaseDisplayMap` and helpers | `view/base_display_map.rs` (ported on `port/engine`; the text view calls it) |
| `Fragments/FragmentBase.swift` | `fragments/fragment_base.rs` |
| `Fragments/InlineCodePill.swift` | `fragments/inline_code_pill.rs` |
| `Motion.swift` | `motion.rs` |
| `Fragments/ListOrnamentFragment.swift` | `fragments/list_ornament_fragment.rs` |
| `Fragments/CodeBlockFragment.swift` | `fragments/code_block_fragment.rs` |
| `Fragments/CalloutFragment.swift` | `fragments/callout_fragment.rs` |
| `Fragments/TableFragment.swift` (`TableCellPresentation`, `TableLayout`, `TableRowFragment`) | `fragments/table_fragment.rs` |
| `Fragments/ImageFragment.swift` | `fragments/image_fragment.rs` |
| `Fragments/LocalAssetPolicy.swift` | `fragments/local_asset_policy.rs` |
| `Fragments/BoundedImageCache.swift` (`MermaidCacheKey`, `MarkdownFragmentImageCaches.images`/`.mermaid`, `ImageRenderCache`) | `fragments/bounded_image_cache.rs`; the generic `BoundedImageCache<Key>` and the math cache are `upleft_math::downright::bounded_image_cache` |
| `Fragments/ThematicBreakFragment.swift` | `fragments/thematic_break_fragment.rs` |
| `Fragments/FrontMatterFragment.swift` | `fragments/front_matter_fragment.rs` |
| `Fragments/MathFragment.swift` (with `DownrightFragment.draw(image:…)`) | `fragments/math_fragment.rs` |
| `Fragments/MermaidFragment.swift`, and the cached door of `MermaidRendererBridge.image(source:styleSheet:)` | `fragments/mermaid_fragment.rs` (the uncached bridge is `upleft_mermaid::downright::mermaid_renderer_bridge`) |
| Swift overlay conveniences (`CGRect.minY`, `NSRect.fill()`, `DispatchWorkItem`, …) | `appkit_compat.rs` |

Objective-C class names equal the Swift ones: `MarkdownTextView`,
`MarkdownContainerView`, `MarkdownContentStorage`, `MarkdownTextParagraph`,
`ParagraphSubstitution`, `FragmentProvider`, `GutterRailView`,
`FootnoteMarginView`, `FragmentAccessibilityElement`, `HeadingMenuAction`,
`DownrightFragment`, `ProseFragment`, `ElidedFragment`, `ElisionCueFragment`,
`SpringSurfaceView`, `DensityGutterView`, `DensityGutterPreviewWindow`,
`PreviewContentView`, `DensityOutlineWindow`, `OutlineTableView`,
`DensityOutlineRow`, `OutlineBackdrop`.

## How the Swift maps

- Swift stored properties are ivars in `Cell`/`RefCell`, borrowed only for
  the statement that uses them: AppKit re-enters the class from inside its
  own calls (a `super` selection change, a storage edit, a layout pass).
- Swift `didSet` properties are `set_…` methods that run the same body.
- `StyleSheet` is shared as `Rc<StyleSheet>` (a Swift value type, copied).
- `DispatchQueue.main.async` / `asyncAfter` / `DispatchWorkItem` are
  `appkit_compat::{main_async, main_after, WorkItem}` — the same libdispatch
  calls, with the closure kept on the main thread.
- The delegate is a Rust trait (`MarkdownTextViewDelegate`) with the Swift
  protocol extension's defaults, held weakly as `Weak<dyn …>`.

## The object-fragment seam

`fragment_provider.rs` keeps the Swift dispatch verbatim and constructs each
object fragment through one function per Swift class, at the bottom of the
file: `code_block_fragment`, `table_row_fragment`, `math_fragment`,
`mermaid_fragment`, `image_fragment`, `front_matter_fragment`,
`thematic_break_fragment`, `callout_fragment`, `list_ornament_fragment`.
Each calls its module's `make`; only `table_row_fragment` can decline
(`TableRowFragment.make` returns `nil` without table data), and the provider
then falls back to `ProseFragment` as Swift does.
`object_geometry::{task_hit_rect, code_copy_button_rect}` are the two static
geometry helpers the view's hit testing borrows (`ListOrnamentFragment.taskHitRect`,
`CodeBlockFragment.copyButtonRect`).

### Things the object fragments do differently from their neighbours

- **Mermaid is reached through a hook.** `upleft-mermaid` depends on this
  crate (for `StyleSheet`), so `mermaid_fragment` cannot call it. Every host
  calls `upleft_mermaid::downright::mermaid_renderer_bridge::install_fragment_renderer()`
  once at start-up (`upleft-oracle` does it in `main`; the app must too).
  Without it every diagram draws as "Diagram could not be rendered".
  `mermaid_fragment::mermaid_image` is Swift's cached
  `MermaidRendererBridge.image(source:styleSheet:)`: trim, `MermaidCacheKey`,
  `MarkdownFragmentImageCaches.mermaid`, then the installed renderer.
- **Threading follows Swift.** Math and Mermaid render synchronously, on the
  main thread, the first time layout asks the fragment for its height, as
  Swift's `overrideHeight` does; that is what puts the formula or diagram in
  the first displayed frame (the `render` suite captures the settled window,
  and Swift's first frame already contains them). Both Rust renderers are
  faster than Swift's (upleft-math ~1.6–2×, the uncached Mermaid bridge
  ~2.7×). Images never block: `ImageRenderCache` decodes on a
  user-initiated global queue and the fragment draws a placeholder until the
  decode lands and invalidates it, exactly as Swift.
- **`FragmentPayload.table_data` is an `Rc<TableData>`**, the cheap snapshot a
  Swift value copy is; each row fragment shares its table's data instead of
  cloning every row.
- **URLs.** `LocalAssetPolicy` runs on `NSURL`. Swift standardizes a relative
  URL after resolving it against its base; `canonical_file_url` takes
  `absoluteURL` first to match (probed; see the module docs).

A Swift subclass of `DownrightFragment` is, in Rust, a `FragmentBehavior`
(the four hooks Swift subclasses override: `vertical_padding`,
`suppresses_text`, `override_height`, `draw_object`) plus the Swift class
name:

```rust
DownrightFragment::new(c"ThematicBreakFragment", request.text_element,
    Some(request.element_range), request.payload, request.context,
    Box::new(ThematicBreak { /* the subclass's own stored properties */ }))
```

`DownrightFragment` implements `layoutFragmentFrame`,
`renderingSurfaceBounds` and `drawAtPoint:inContext:` once and dispatches to
the behaviour; the instance is allocated from a runtime subclass registered
under the given name, so the layout dump reads `ThematicBreakFragment`.
Helpers the Swift subclasses call on `self` are methods on
`DownrightFragment`: `style_sheet`, `style_token`, `content_width`,
`prose_content_width`, `bounds`, `source_text`, `paragraph_style`,
`element_source_range`, `is_first_paragraph_of_block`, `super_layout_fragment_frame`,
`failed_object_height`, `draw_failed_object`; the free helpers are
`clipped`, `mixed`, `draw_ns_image`, `fill_rect`, `fill_rect_corners`,
`draw_text`, `RectCorners`, `FailedObject`. `FragmentContext` carries
everything Swift's does; `table_layouts` stores the table fragment's
`TableLayout` type-erased (`Rc<dyn Any>`). `tests/view_tests/fragment_seam_tests.rs`
pins the mechanism.

## The density gutter

`DensityGutterView` is a `define_class!` subclass of the ported
`SpringSurfaceView` (`#[unsafe(super(SpringSurfaceView, NSView, NSResponder))]`)
and overrides `springTick:`, `springApply` and `springsSettleImmediately` as
Objective-C methods, so the base driver dispatches to it the way Swift's
`open` methods do. Swift's `didSet` properties are `set_…` methods
(`set_bands`, `set_visible_range` as a `(lower, upper)` pair, …);
`DensityGutterDelegate` is a Rust trait held weakly. The Swift test hooks
are public: `drive_hover_for_testing`, `mark_positions_for_testing`,
`set_perform_haptic_feedback`, `PipSimulation`. `MarkdownContainerView`
recognises the gutter by type and calls `container_geometry_did_change`.
The preview card and the outline panel are the gutter's child windows
(`preview_window()`, `outline_window()`).

Conformance (2026-09-23; the screen was locked for the whole session, so
ScreenCaptureKit could not run and the two windowed suites were run with
`--capture view` appended to every variant, a temporary edit of
`suites.json`):

- `density-model` (windowless: bands with synthetic change and search
  overlays, thinning, pips, stack geometry, hover sweep, the mark layers in
  each driven state, click and scrub hit testing, hand-stepped spring
  frames, outline rows): 1818/1818 (909 documents × {Paper Light, Nord
  dark}).
- `render-density` (`render --density leading|trailing`, gated to the 563
  documents whose live decoration carries no object-fragment payload):
  1126/1126 with `--capture view`. Screen capture: not run (unverified).
- `density-hover` (40 documents, Source mode so no object fragments,
  1400×1000; leading 0.25/0.5/0.75/outline, trailing dark 0.5/outline):
  240/240 with `--capture view`. Screen capture: not run (unverified).
  One case first failed because the Swift oracle was rebuilt, unstamped,
  while the run was in progress; re-run, it passes.

To run them: `just conform --suite density-model` (any time),
`--suite render-density`, `--suite density-hover` (need an unlocked,
awake display). Once the object fragments land, drop `render-density`'s
`only` list and switch `density-hover` to Live mode.

`bench-density` (both oracles, agent-5000.md, 1160 bands, 18 marks,
windowless, 200 runs after 20 warm-up, best p50 of three interleaved
rounds):

| stage | Swift | Rust |
|---|---:|---:|
| `bands(for:)` | 45.6 µs | 17.4 µs |
| `bands(for:)` with overlays | 47.8 µs | 17.8 µs |
| `selection(for:capacity:)` | 7.9 µs | 4.8 µs |
| selection with pips | 15.3 µs | 12.0 µs |
| `bands` assignment (selection + layers) | 29.6 µs | 25.6 µs |
| redraw (`layout`) | 21.7 µs | 19.5 µs |
| hover step | 24.2 µs | 22.1 µs |

## Tests

`cargo test -p upleft-render --test view_tests` runs the view-level
MarkdownRenderTests in a main-thread harness (98 tests: ClickStability,
ContentResize, LayoutFiller, SpeechAccessibility, SmartPasteIntegration,
DropAndQuickLook, DensityRail (all 33), plus the fragment seam). `content_storage_tiling_tests`,
`motion_system_tests` (including `delayedSpringsReleaseOnTheirOwn`, which
drives `PipSimulation`) and `geometry_probe_tests` are ordinary test binaries
(GeometryProbeTests' two `MathRenderer` tests live in `upleft-math`). Not ported, because
it needs unported code: `ClickStabilityTests.checkboxDoubleClickDoesNotToggleTwice`
(`ListOrnamentFragment.taskHitRect`).
MarkdownRenderTests in a main-thread harness: ClickStability (including
`checkboxDoubleClickDoesNotToggleTwice`), ContentResize, LayoutFiller,
SpeechAccessibility, SmartPasteIntegration, DropAndQuickLook, ListOrnament,
CalloutGeometry, CodeBlockGeometry, TypingInvalidation, the `ImageRenderCache`
cases of BoundedImageCacheTests, the `@MainActor` DecorationTests cases that
need the view or the content storage (`decoration_view_tests.rs`), plus the
fragment seam. `content_storage_tiling_tests`, `motion_system_tests`,
`geometry_probe_tests`, `local_asset_policy_tests` and `decoration_tests`
(which now includes its table-layout, task-hit-rect and copy-button cases)
are ordinary test binaries. GeometryProbeTests' two `MathRenderer` tests,
MathFontBundleTests and BoundedImageCacheTests.limitsAreEnforced live in
`upleft-math`; MermaidOrientationProbeTests in `upleft-mermaid`, which also
checks the fragment layer's cached door. Not ported, because they need
unported code: `SpeechAccessibilityTests`' `DensityGutterView` assertions,
`MotionSystemTests.delayedSpringsReleaseOnTheirOwn` (`DensityGutterView.PipSimulation`).

## Left

- The object fragments (see the seam above).
- `render` conformance for documents that use object fragments waits on
  those ports.
- `DensityGutterView`, `DensityGutterPreviewWindow`, `DensityOutlineWindow`.
  `MarkdownContainerView` recognises a density gutter accessory by its
  Objective-C class name (`DensityGutterView`) and calls
  `containerGeometryDidChange` on it by selector.

## Conformance and performance (2026-09-23)

### Object fragments (port/fragments)

The machine's screen was locked for the whole fragment session, so
ScreenCaptureKit refused every window capture (`SCStreamErrorDomain -3811`)
and the `render` suite itself could not run after the first three documents
(agent-40 and agent-400 passed all four variants before the lock). Source
mode renders no objects and was identical before this port. The
oracles' `--capture view` path (`cacheDisplay`, the same scene, settle loop
and layout dump) still works on a locked screen, so the port was checked with
it, both oracles, document by document:

| run | cases | identical (layout dump and PNG) |
|---|---:|---:|
| whole corpus, `--mode live` | 909 | 909 |
| whole corpus, `--mode live --dark` | 909 | 909 |
| whole corpus, `--mode live --width 1400 --height 1000` | 909 | 909 |
| 50 fragment-heavy documents + `render-images`, live / dark / wide / Nord / Warm Dark / source | 285 | 285 |

**Unverified until the screen is unlocked:** `just conform --suite render`
(screen capture, 3636 cases), `--suite render-dark-themes`, `--suite
render-images` and `--suite probe`.

New suites: `render-dark-themes` (50 documents that between them use every
object fragment, in Nord and in Warm Dark) and `render-images`
(`corpus/render-images/*.markdown`, read by no other suite, with real local
images; `scripts/build-render-images-corpus.py` regenerates it). Both render
scenes now set `documentURL` to the input, as the app does, so relative
images go through `LocalAssetPolicy` and the background loader.

`bench-view` with objects drawing (10 runs, best p50 of three interleaved
rounds, screen locked), `update(document:)` to settled:

| document | Swift | Rust | Rust / Swift |
|---|---:|---:|---:|
| agent-5000, Live (like for like: objects draw on both sides) | 258.1 ms | 200.8 ms | 0.78 |
| agent-5000, Source | 357.9 ms | 307.3 ms | 0.86 |
| README (tables, code) | 16.8 ms | 14.2 ms | 0.84 |
| Docs__FEATURE-MATRIX (tables) | 10.5 ms | 7.9 ms | 0.75 |
| Docs__PERFORMANCE (tables, code) | 8.3 ms | 6.8 ms | 0.82 |
| Docs__sample (math, Mermaid, callouts, tasks) | 6.2 ms | 5.5 ms | 0.89 |

`just bench`: all 21 drbench stages as fast or faster.

### View layer (port/view)

`render`: every document whose blocks need only prose, elided or cue
fragments (558 of 909) is identical in all four variants (2232/2232 cases,
pixels and layout dump), and every document is identical in Source mode
(351/351 more, since Source mode renders no objects).

`bench-view` (both oracles, 10 runs, best p50 of three interleaved rounds,
on a loaded machine), time from `update(document:)` to settled:

| document | Swift | Rust |
|---|---|---|
| agent-5000, Source mode (like for like) | 384.5 ms | 324.0 ms |
| synthetic prose, 5833 lines, Live (like for like) | 279.5 ms | 225.2 ms |
| agent-5000, Live (Rust drew objects as prose then) | 275.7 ms | 223.6 ms |

`update(document:)` alone is 37–41% faster; the settle pass (TextKit
laying out the document) is 2–4% faster.
