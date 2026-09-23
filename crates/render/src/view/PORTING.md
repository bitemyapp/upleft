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
`SpringSurfaceView`.

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

## Tests

`cargo test -p upleft-render --test view_tests` runs the view-level
MarkdownRenderTests in a main-thread harness (65 tests: ClickStability,
ContentResize, LayoutFiller, SpeechAccessibility, SmartPasteIntegration,
DropAndQuickLook, plus the fragment seam). `content_storage_tiling_tests`,
`motion_system_tests` and `geometry_probe_tests` are ordinary test binaries
(GeometryProbeTests' two `MathRenderer` tests live in `upleft-math`). Not ported, because
they need unported code: `ClickStabilityTests.checkboxDoubleClickDoesNotToggleTwice`
(`ListOrnamentFragment.taskHitRect`), `SpeechAccessibilityTests`'
`DensityGutterView` assertions, `MotionSystemTests.delayedSpringsReleaseOnTheirOwn`
(`DensityGutterView.PipSimulation`).

## Left

- The object fragments (see the seam above) and `DensityGutterView`,
  `DensityGutterPreviewWindow`, `DensityOutlineWindow`.
  `MarkdownContainerView` recognises a density gutter accessory by its
  Objective-C class name (`DensityGutterView`) and calls
  `containerGeometryDidChange` on it by selector.
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
