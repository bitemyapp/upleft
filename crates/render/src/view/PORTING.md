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
Each returns `None` today and the provider falls back to `ProseFragment`.
`object_geometry::{task_hit_rect, code_copy_button_rect}` are the two static
geometry helpers the view's hit testing borrows (`ListOrnamentFragment.taskHitRect`,
`CodeBlockFragment.copyButtonRect`); they return `None` until ported, which
disables those hit targets.

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

## Left

- The object fragments (see the seam above).
- `render` conformance for documents that use object fragments waits on
  those ports.

## Conformance and performance (2026-09-23)

`render`: every document whose blocks need only prose, elided or cue
fragments (558 of 909) is identical in all four variants (2232/2232 cases,
pixels and layout dump), and every document is identical in Source mode
(351/351 more, since Source mode renders no objects). The remaining 1053
cases need the object fragments.

`bench-view` (both oracles, 10 runs, best p50 of three interleaved rounds,
on a loaded machine), time from `update(document:)` to settled:

| document | Swift | Rust |
|---|---|---|
| agent-5000, Source mode (like for like) | 384.5 ms | 324.0 ms |
| synthetic prose, 5833 lines, Live (like for like) | 279.5 ms | 225.2 ms |
| agent-5000, Live (Rust draws objects as prose) | 275.7 ms | 223.6 ms |

`update(document:)` alone is 37–41% faster; the settle pass (TextKit
laying out the document) is 2–4% faster.
