# Known differences from Downright

This ledger lists every place where Upleft is known or suspected to behave differently from the Swift original. An entry leaves this file only when its difference is fixed and a conformance case covers it, or when the entry is recorded as an intentional, reviewed deviation.

## Open

| Area | Difference | Found by | Plan |
|---|---|---|---|
| Theme JSON decoding (`upleft-render` `swift_compat`) | Swift's `JSONDecoder` rejects some zero literals, such as `0e1` and `0.00e-5`, and the port accepts them. This only affects malformed theme files. | render-base port | Find Foundation's number grammar with a probe covering every literal shape, then match it. Add a malformed-theme corpus to the `vscode-theme` suite. |
| `upleft-math` `add_latex_symbol` across threads | The symbol table stores the registered atom itself, as Swift does, so a later change to it shows in lookups on the thread that registered it. Another thread gets a copy of the atom as it was when registered, because Rust can't share the caller's non-thread-safe cells. In Swift that cross-thread read is a data race unless the caller synchronises. Downright never calls `add(latexSymbol:)`. | math-variants | Keep. Covered by `crates/math/tests/add_latex_symbol_tests.rs`. |
| ELK layering where elk-swift is nondeterministic (`upleft-elk`) | elk-swift iterates `NetworkSimplex.treeEdges`, a `Set` hashed by object address with a per-process seed, and breaks `HyperEdgeCycleDetector` ties the same way, so on some graphs its layout varies from run to run. None of the graphs captured from real Mermaid diagrams vary; 50 of the 449 corpus graphs (random and polyline stress graphs) do. Upleft always takes the insertion-order outcome. On 37 of the 50 that equals one of 16 sampled elk-swift runs. On 13 it equals only the instrumented elk-swift copy (`just elklab`), and whether plain elk-swift can produce those outcomes is unverified. The `elk` oracle emits `{"alternatives": [...]}` only for graphs whose samples disagree, and consults the instrumented copy only then. | elk port | Keep. Downright has no single output to match on these graphs. Watch for any Mermaid-derived graph that varies. |
| ELK compound preprocessor | The port clears the port-to-node entry list at the start of every layout; Swift never clears it. The stale entries only touch graphs from earlier layouts, so the output is identical. | elk port | Keep. |
| `DocumentHealth` footnote order | Swift walks footnotes in `Dictionary` order, which changes from run to run; the port walks them by position. No real parse can show the difference. | core port | Keep. Swift has no fixed order to match. |
| Atomic save temp names | Foundation's `.sb-` temp-file naming is only approximated, and its fallback for a directory that isn't writable is not ported. | core port | Port the naming exactly when the app layer lands. Check by saving into a directory that isn't writable. |
| Sorting by Swift `String <` (`upleft-swift-text` `str_cmp`) | Swift's `<` is not a strict weak order on strings that are not in NFC: when one string's NFC is a prefix of the other's, the string with fewer UTF-8 bytes is less, so `"a\u{301}"` and `"\u{E1}b"` are neither less nor equal. `str_less` reproduces `<` exactly (the `unicode` suite checks it), but Rust's `sort_by` over `str_cmp` can order such keys differently from Swift's sort, or panic where Rust detects the inconsistency. Real keys (link labels, identifiers, theme names) do not hit this. | swift-text | Port Swift's `sort(by:)` (its insertion sort and merge) into `upleft-swift-text` and use it wherever a port sorts by a `String` comparison. |
| `String` comparison on bridged strings | `str_less` models native Swift strings. A string bridged from an `NSString` skips the byte-prefix shortcut and compares remaining lengths in UTF-16 units, so the same two strings can order differently when one side is bridged. Not observed in any port. | swift-text | Add a `str_less_bridged` when a call site is found that compares a bridged string with `<`. |
| `ThemeStore` hot reload | Rust reads the themes folder on a background queue during a hot reload. Swift reads it on the main thread. The loaded result is the same; only when it lands can differ. | render-base port | Intentional, per AGENTS.md: never block the main thread. The observable state after a reload must still match. |

## Reproduced on purpose

These are Downright behaviours that look like bugs. Upleft reproduces them because pixel-exactness requires it.

| Behaviour | Where |
|---|---|
| `SmartPaste.plainText(forHTML:)` never returns on input such as `a < b`. The port reproduces the hang. | `upleft-core` SmartPaste |
| swift-markdown measures every table row as the width of the first row when it pads column alignments. cmark never produces rows of different widths, so real input is unaffected. | `upleft-markup` tables |

## Retracted

- **"Downright clips the text column on first open."** This was never Downright's behaviour. It came from SwiftPM stamping the binary `sdk 14.0`, which put AppKit into an older compatibility mode. With the canonical build version (docs/BUILD-VERSION.md), the text view takes its full width.
