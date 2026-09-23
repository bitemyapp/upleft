# Known differences from Downright

This ledger lists every place where Upleft is known or suspected to behave differently from the Swift original. An entry leaves this file only when its difference is fixed and a conformance case covers it, or when the entry is recorded as an intentional, reviewed deviation.

## Open

| Area | Difference | Found by | Plan |
|---|---|---|---|
| Theme JSON decoding (`upleft-render` `swift_compat`) | Swift's `JSONDecoder` rejects some zero literals, such as `0e1` and `0.00e-5`, and the port accepts them. This only affects malformed theme files. | render-base port | Find Foundation's number grammar with a probe covering every literal shape, then match it. Add a malformed-theme corpus to the `vscode-theme` suite. |
| Unicode tables (`upleft-markup`, `upleft-render`) | Grapheme segmentation, normalization, and `isLetter`/`isNumber` come from Rust crates whose Unicode version may differ from Swift 6.4's. | markup and render-base ports | Add a `unicode` suite that checks every scalar and the Unicode grapheme break tests through both oracles. Merge the per-crate Swift-compatibility helpers into one crate. |
| `ThemeStore` hot reload | Rust reads the themes folder on a background queue during a hot reload. Swift reads it on the main thread. The loaded result is the same; only when it lands can differ. | render-base port | Intentional, per AGENTS.md: never block the main thread. The observable state after a reload must still match. |

## Reproduced on purpose

These are Downright behaviours that look like bugs. Upleft reproduces them because pixel-exactness requires it.

| Behaviour | Where |
|---|---|
| swift-markdown measures every table row as the width of the first row when it pads column alignments. cmark never produces rows of different widths, so real input is unaffected. | `upleft-markup` tables |

## Retracted

- **"Downright clips the text column on first open."** This was never Downright's behaviour. It came from SwiftPM stamping the binary `sdk 14.0`, which put AppKit into an older compatibility mode. With the canonical build version (docs/BUILD-VERSION.md), the text view takes its full width.
