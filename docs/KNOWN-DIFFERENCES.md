# Known differences from Downright

This ledger lists every place where Upleft is known or suspected to behave differently from the Swift original. An entry leaves this file only when its difference is fixed and a conformance case covers it, or when the entry is recorded as an intentional, reviewed deviation.

## Open

| Area | Difference | Found by | Plan |
|---|---|---|---|
| Theme JSON decoding (`upleft-render` `swift_compat`) | Swift's `JSONDecoder` rejects some zero literals, such as `0e1` and `0.00e-5`, and the port accepts them. This only affects malformed theme files. | render-base port | Find Foundation's number grammar with a probe covering every literal shape, then match it. Add a malformed-theme corpus to the `vscode-theme` suite. |
| `upleft-math` `add_latex_symbol` across threads | The symbol table stores the registered atom itself, as Swift does, so a later change to it shows in lookups on the thread that registered it. Another thread gets a copy of the atom as it was when registered, because Rust can't share the caller's non-thread-safe cells. In Swift that cross-thread read is a data race unless the caller synchronises. Downright never calls `add(latexSymbol:)`. | math-variants | Keep. Covered by `crates/math/tests/add_latex_symbol_tests.rs`. |
| `.drBlock` and `.drPathToken` values (`upleft-render` `swift_value`) | Swift stores `BlockIdentity` and `PathToken` in `__SwiftValue` boxes; the port uses the NSObject subclasses `UpleftBlockIdentityValue` and `UpleftPathTokenValue`. `isEqual:` matches Swift's `Hashable` equality, so attribute runs coalesce identically (decorate 5454/5454), but the class names, `-description` and `-hash` values differ. | engine port | Keep unless something reads the class name. |
| Lone surrogates in the decoration engine | Swift reads a code block's text and a callout's source through `String`, which repairs a lone surrogate to U+FFFD before highlighting or searching; the port passes the UTF-16 units as they are. Unverified; no corpus document has a lone surrogate. | engine port | Add a lone-surrogate document to the corpus and compare. |
| `ClipboardSemanticHTML` substring search | Swift's `Substring.range(of:)` is Foundation's non-literal search; the port matches Character by Character, with Swift's canonical equivalence for single characters. The two agree on the corpus (clipboard 909/909). Unverified next to composed sequences such as `*` followed by a combining mark. | engine port | Add clipboard cases with combining marks after `*`, `_`, `~` and backticks. |
| `DecorationResult.elapsed` | Measured with a monotonic clock; Swift subtracts `CFAbsoluteTimeGetCurrent()` readings, which follow wall-clock changes. Only the timing differs. | engine port | Intentional. |
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
