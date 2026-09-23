# Known differences from Downright

This ledger lists every place where Upleft is known or suspected to behave differently from the Swift original. An entry leaves this file only when its difference is fixed and a conformance case covers it, or when the entry is recorded as an intentional, reviewed deviation.

## Open

| Area | Difference | Found by | Plan |
|---|---|---|---|
| Theme JSON decoding (`upleft-render` `swift_compat`) | Swift's `JSONDecoder` rejects some zero literals, such as `0e1` and `0.00e-5`, and the port accepts them. This only affects malformed theme files. | render-base port | Find Foundation's number grammar with a probe covering every literal shape, then match it. Add a malformed-theme corpus to the `vscode-theme` suite. |
| Unicode tables (`upleft-markup`, `upleft-render`) | These crates still get grapheme segmentation, normalization and `isLetter`/`isNumber` from Rust crates. `unicode-segmentation` is confirmed to disagree with Swift 6.4: Swift treats only six Indic viramas as linking conjuncts, and it doesn't treat the Kirat Rai vowel signs as Hangul-like vowels. `upleft-core`'s `swift_text` reimplements Swift's grapheme state machine from tables generated out of the Swift runtime, and it matched on 33.4M context checks. | core port | Move `swift_text` into a shared crate and switch `upleft-markup` and `upleft-render` to it. Add a `unicode` suite that checks every scalar through both oracles. |
| `upleft-math` `add_latex_symbol` | Only plain, operator, space and style atoms can be registered; Swift stores any atom. Downright never calls it. | math port | Store any atom, matching Swift. |
| `upleft-math` coverage | The math suites only render Paper Light, light appearance. The text colour and `mathPointSize` change with theme and appearance. | math port | Add theme and appearance variants to `math-image` and `math-tree`. |
| `DocumentHealth` footnote order | Swift walks footnotes in `Dictionary` order, which changes from run to run; the port walks them by position. No real parse can show the difference. | core port | Keep. Swift has no fixed order to match. |
| Atomic save temp names | Foundation's `.sb-` temp-file naming is only approximated, and its fallback for a directory that isn't writable is not ported. | core port | Port the naming exactly when the app layer lands. Check by saving into a directory that isn't writable. |
| `upleft-markup` `swift_contains_character` | Returns early on a byte search, so it misses canonically equivalent characters (for example U+1FEF, which Swift treats as equal to a backtick). It also uses `unicode-segmentation`, which is known to differ from Swift 6.4. Unverified. | core port | Switch it to `upleft-core`'s grapheme implementation, which is generated from the Swift runtime; see the Unicode row. |
| `ThemeStore` hot reload | Rust reads the themes folder on a background queue during a hot reload. Swift reads it on the main thread. The loaded result is the same; only when it lands can differ. | render-base port | Intentional, per AGENTS.md: never block the main thread. The observable state after a reload must still match. |

## Reproduced on purpose

These are Downright behaviours that look like bugs. Upleft reproduces them because pixel-exactness requires it.

| Behaviour | Where |
|---|---|
| `SmartPaste.plainText(forHTML:)` never returns on input such as `a < b`. The port reproduces the hang. | `upleft-core` SmartPaste |
| swift-markdown measures every table row as the width of the first row when it pads column alignments. cmark never produces rows of different widths, so real input is unaffected. | `upleft-markup` tables |

## Retracted

- **"Downright clips the text column on first open."** This was never Downright's behaviour. It came from SwiftPM stamping the binary `sdk 14.0`, which put AppKit into an older compatibility mode. With the canonical build version (docs/BUILD-VERSION.md), the text view takes its full width.
