# Known differences from Downright

This ledger lists every place where Upleft is known or suspected to behave differently from the Swift original. An entry leaves this file only when its difference is fixed and a conformance case covers it, or when the entry is recorded as an intentional, reviewed deviation.

## Open

| Area | Difference | Found by | Plan |
|---|---|---|---|
| Theme JSON decoding (`upleft-render` `swift_compat`) | Swift's `JSONDecoder` rejects some zero literals, such as `0e1` and `0.00e-5`, and the port accepts them. This only affects malformed theme files. | render-base port | Find Foundation's number grammar with a probe covering every literal shape, then match it. Add a malformed-theme corpus to the `vscode-theme` suite. |
| `upleft-math` `add_latex_symbol` across threads | The symbol table stores the registered atom itself, as Swift does, so a later change to it shows in lookups on the thread that registered it. Another thread gets a copy of the atom as it was when registered, because Rust can't share the caller's non-thread-safe cells. In Swift that cross-thread read is a data race unless the caller synchronises. Downright never calls `add(latexSymbol:)`. | math-variants | Keep. Covered by `crates/math/tests/add_latex_symbol_tests.rs`. |
| `DocumentHealth` footnote order | Swift walks footnotes in `Dictionary` order, which changes from run to run; the port walks them by position. No real parse can show the difference. | core port | Keep. Swift has no fixed order to match. |
| Atomic save temp names | Foundation's `.sb-` temp-file naming is only approximated, and its fallback for a directory that isn't writable is not ported. | core port | Port the naming exactly when the app layer lands. Check by saving into a directory that isn't writable. |
| Sorting by Swift `String <` (`upleft-swift-text` `str_cmp`) | Swift's `<` is not a strict weak order on strings that are not in NFC: when one string's NFC is a prefix of the other's, the string with fewer UTF-8 bytes is less, so `"a\u{301}"` and `"\u{E1}b"` are neither less nor equal. `str_less` reproduces `<` exactly (the `unicode` suite checks it), but Rust's `sort_by` over `str_cmp` can order such keys differently from Swift's sort, or panic where Rust detects the inconsistency. Real keys (link labels, identifiers, theme names) do not hit this. | swift-text | Port Swift's `sort(by:)` (its insertion sort and merge) into `upleft-swift-text` and use it wherever a port sorts by a `String` comparison. |
| `String` comparison on bridged strings | `str_less` models native Swift strings. A string bridged from an `NSString` skips the byte-prefix shortcut and compares remaining lengths in UTF-16 units, so the same two strings can order differently when one side is bridged. Not observed in any port. | swift-text | Add a `str_less_bridged` when a call site is found that compares a bridged string with `<`. |
| `ThemeStore` hot reload | Rust reads the themes folder on a background queue during a hot reload. Swift reads it on the main thread. The loaded result is the same; only when it lands can differ. | render-base port | Intentional, per AGENTS.md: never block the main thread. The observable state after a reload must still match. |
| Sequence diagrams: open activations (`upleft-mermaid` `src_sequence_layout`) | Activations still open after the last message are appended in Swift `Dictionary` order, which is randomised per process, so Swift's own order varies between runs. Upleft appends them in first-activation order. Bars of different participants never overlap, so pixels are unaffected. The `mermaid-layout` dump compares activations sorted. | mermaid port | Intentional: Swift has no stable order to match. |
| Mermaid ELK-backed layouts (`upleft-mermaid`) | Flowcharts, state, class and ER diagrams need `upleft-elk`, which is not linked yet (feature `elk`); without it Upleft returns no image where Downright draws one. The conformance suites report these cases as not ported. | mermaid port | Enable the `elk` feature once `upleft-elk` is merged and green, then run `mermaid-layout` and `mermaid` on the whole corpus. |

## Reproduced on purpose

These are Downright behaviours that look like bugs. Upleft reproduces them because pixel-exactness requires it.

| Behaviour | Where |
|---|---|
| `SmartPaste.plainText(forHTML:)` never returns on input such as `a < b`. The port reproduces the hang. | `upleft-core` SmartPaste |
| swift-markdown measures every table row as the width of the first row when it pads column alignments. cmark never produces rows of different widths, so real input is unaffected. | `upleft-markup` tables |
| An xychart whose values or range approach `1e300` overflows its nice ticks to infinity, and the renderer's `Int((xBase / 20).rounded())` traps: Downright crashes on such a fence. Upleft panics at the same point. The two inputs live in `corpus/mermaid-traps/` (read by no suite); a test checks the panic. | `upleft-mermaid` `diagram_renderer_xy_chart` |
| beautiful-mermaid's flowchart clipping compares shapes against `"stateStart"`/`"stateEnd"`/`"stateFork"`, but positioned nodes carry `"state-start"`/`"state-end"`, so edges into start and end states are clipped as ellipses. | `upleft-mermaid` `src_layout` |
| After shifting a flowchart right or down to fit an edge label, only the top-level subgroup rectangles move; nested subgraph rectangles keep their old coordinates. | `upleft-mermaid` `src_layout` |
| `BMColor(hex:)` treats any length other than 6 or 8 hex digits as opaque black, so `style A fill:#f9f` and named colours such as `red` fill black. | `upleft-mermaid` `cross_platform` |

## Retracted

- **"Downright clips the text column on first open."** This was never Downright's behaviour. It came from SwiftPM stamping the binary `sdk 14.0`, which put AppKit into an older compatibility mode. With the canonical build version (docs/BUILD-VERSION.md), the text view takes its full width.
