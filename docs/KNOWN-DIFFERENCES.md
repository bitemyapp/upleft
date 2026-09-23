# Known differences from Downright

This ledger lists every place where Upleft is known or suspected to behave differently from the Swift original. An entry leaves this file only when its difference is fixed and a conformance case covers it, or when the entry is recorded as an intentional, reviewed deviation.

## Open

| Area | Difference | Found by | Plan |
|---|---|---|---|
| Theme JSON decoding (`upleft-render` `swift_compat`) | Swift's `JSONDecoder` rejects some zero literals, such as `0e1` and `0.00e-5`, and the port accepts them. This only affects malformed theme files. | render-base port | Find Foundation's number grammar with a probe covering every literal shape, then match it. Add a malformed-theme corpus to the `vscode-theme` suite. |
| Unicode tables (`upleft-markup`, `upleft-render`) | Grapheme segmentation, normalization, and `isLetter`/`isNumber` come from Rust crates whose Unicode version may differ from Swift 6.4's. | markup and render-base ports | Add a `unicode` suite that checks every scalar and the Unicode grapheme break tests through both oracles. Merge the per-crate Swift-compatibility helpers into one crate. |
| `ThemeStore` hot reload | Rust reads the themes folder on a background queue during a hot reload. Swift reads it on the main thread. The loaded result is the same; only when it lands can differ. | render-base port | Intentional, per AGENTS.md: never block the main thread. The observable state after a reload must still match. |
| Sequence diagrams: open activations (`upleft-mermaid` `src_sequence_layout`) | Activations still open after the last message are appended in Swift `Dictionary` order, which is randomised per process, so Swift's own order varies between runs. Upleft appends them in first-activation order. Bars of different participants never overlap, so pixels are unaffected. The `mermaid-layout` dump compares activations sorted. | mermaid port | Intentional: Swift has no stable order to match. |
| Mermaid ELK-backed layouts (`upleft-mermaid`) | Flowcharts, state, class and ER diagrams need `upleft-elk`, which is not linked yet (feature `elk`); without it Upleft returns no image where Downright draws one. The conformance suites report these cases as not ported. | mermaid port | Enable the `elk` feature once `upleft-elk` is merged and green, then run `mermaid-layout` and `mermaid` on the whole corpus. |

## Reproduced on purpose

These are Downright behaviours that look like bugs. Upleft reproduces them because pixel-exactness requires it.

| Behaviour | Where |
|---|---|
| On first open the text view keeps its initial width (for example 404–504 pt) while its text container is 592 pt, so the text column is clipped on the right until something resizes the view. This was confirmed in the real Downright.app at `9be0680`. | `MarkdownContainerView` / `MarkdownTextView` first frame |
| swift-markdown measures every table row as the width of the first row when it pads column alignments. cmark never produces rows of different widths, so real input is unaffected. | `upleft-markup` tables |
| An xychart whose values or range approach `1e300` overflows its nice ticks to infinity, and the renderer's `Int((xBase / 20).rounded())` traps: Downright crashes on such a fence. Upleft panics at the same point. The two inputs live in `corpus/mermaid-traps/` (read by no suite); a test checks the panic. | `upleft-mermaid` `diagram_renderer_xy_chart` |
| beautiful-mermaid's flowchart clipping compares shapes against `"stateStart"`/`"stateEnd"`/`"stateFork"`, but positioned nodes carry `"state-start"`/`"state-end"`, so edges into start and end states are clipped as ellipses. | `upleft-mermaid` `src_layout` |
| After shifting a flowchart right or down to fit an edge label, only the top-level subgroup rectangles move; nested subgraph rectangles keep their old coordinates. | `upleft-mermaid` `src_layout` |
| `BMColor(hex:)` treats any length other than 6 or 8 hex digits as opaque black, so `style A fill:#f9f` and named colours such as `red` fill black. | `upleft-mermaid` `cross_platform` |
