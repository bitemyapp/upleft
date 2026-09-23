# Upleft

Upleft is a Rust rewrite of [Downright](https://github.com/ezzy1630/Downright), the native macOS Markdown reader and editor. It uses AppKit and TextKit 2 through [objc2](https://github.com/madsmtm/objc2), the way [Omperor](https://github.com/bitemyapp/omperor) does.

The goal is strict: **the same output as Downright, pixel for pixel, at the same speed or faster.** Every layer is checked against the Swift original, which is built from source out of `vendor/downright`.

## Status

The layers below are ported in this order. A layer counts as done only when the harness agrees with the original on it.

| Layer | Swift source | Rust crate | Conformance gate | Status |
|---|---|---|---|---|
| cmark-gfm (C, not rewritten) | `swiftlang/swift-cmark` @ `7898f1b` | `upleft-cmark-gfm-sys` | links the identical C sources | done |
| swift-markdown converter | `apple/swift-markdown` @ `27b7fc1` | `upleft-markup` | syntax-tree dump identical | **done**: 897/897 corpus documents identical; 5,000-line document parses in 2.1 ms vs 15.3 ms in Swift |
| MarkdownCore | `Sources/MarkdownCore` | `upleft-core` | `parse` dump identical | **done**: parse 909/909 corpus documents identical (plus 14/14 UTF-8 edge cases); the text-level functions (`core-text`, `core-io`) are identical on 909/909 documents and 33/33 byte-level edge cases. The 5,000-line document parses in 6.3 ms vs 29.9 ms in Swift, and every other benchmarked stage is at least as fast. |
| MarkdownRender | `Sources/MarkdownRender` | `upleft-render` | `decorate` dump and `render` pixels identical | in progress. Done: decoration engine and display map (decorate 5454/5454 across 6 variants, incremental 2727/2727, displaymap 2727/2727, clipboard 909/909), stylesheet 12/12, syntax highlight 909/909, vscode-theme 19/19. Still to do: the text view and the object fragments. |
| SwiftMath | `Vendor/SwiftMath` | `upleft-math` | math pixels identical | **done**: math-image and math-tree each 15420/15420 identical (1285 inputs × 6 themes × light and dark); parse, typeset and render together take 137–175 ms vs 263–278 ms in Swift |
| beautiful-mermaid + ELK | `lukilabs/*` | `upleft-mermaid`, `upleft-elk` | diagram pixels identical | **done**. ELK: elk 449/449 layouts identical (Mermaid-generated, elk-swift test, random and polyline graphs; where elk-swift itself varies between runs, Rust matches one of its outcomes), 6.6–14× faster than elk-swift. Mermaid, on 308 corpus diagrams of every type: mermaid-parse 308/308, mermaid-layout 1232/1232 and mermaid (bridge pixels) 1232/1232 across light, dark, Nord and High Contrast; mermaid-replay 211/211 (ELK inputs byte-identical to Swift's). The uncached bridge path is about 2.7× faster than Swift. |
| DownrightApp and the command-line tools | `Sources/DownrightApp`, `drdownright`, `down`, `DownrightSpotlightMetadata`, `DownrightQL` | `upleft-app`, `upleft-cli`, `upleft-spotlight-metadata`, `upleft-quicklook`, `upleft-foundation` | app-layer suites identical; window captures identical | in progress. Done: the non-UI layer and the command-line tools. html-export 13706/13706 (6 themes × light and dark, plus print), spotlight 928/928, down-cli 244/244 (the real `down` against Upleft's, in sandboxed homes), workspace 6/6, find 4/4, palette 23/23, formats 30/30, updater 10/10, local-ai 4/4. All 15 `just app-bench` stages are as fast or faster (HTML export of the 5,000-line document: 0.58 ms vs 12.3 ms). Still to do: the windows, panels and menus (UI port), and Sparkle linkage (packaging). See `crates/app/PORTING.md`. |

## Performance

`just bench` runs Downright's own `drbench` and Upleft's `upleft-bench` three times each, interleaved, and compares the median p50 of every stage. It fails if any stage is slower than Swift by more than 5% (a run-to-run noise allowance). Latest run on an Apple Silicon MacBook Pro, milliseconds:

| Stage | Swift p50 | Upleft p50 | Upleft / Swift |
|---|---:|---:|---:|
| cmark alone (5k lines) | 11.5 | 1.6 | 0.14 |
| MarkdownParser.parse, all passes | 25.3 | 5.2 | 0.21 |
| parse 100 KB (cold open) | 21.1 | 3.9 | 0.18 |
| incremental decorate, one dirty block | 0.086 | 0.051 | 0.59 |
| wholesale decorate (mode switch) | 103.6 | 45.6 | 0.44 |
| edit + paragraph map (typing response) | 0.138 | 0.080 | 0.58 |
| worker pipeline (end-to-end convergence) | 25.7 | 5.4 | 0.21 |
| Metrics.metrics | 24.1 | 5.7 | 0.24 |
| syntax highlight 10 KB | 0.082 | 0.051 | 0.62 |

All 21 drbench stages are as fast or faster. The math, Mermaid and ELK layers have their own benchmarks; see each crate's `PORTING.md` and the status table above.

## Layout

```text
vendor/downright            the Swift original (git submodule, pinned)
vendor/swift-cmark          cmark-gfm at the revision Downright resolves
vendor/swift-markdown       swift-markdown at the revision Downright resolves
vendor/beautiful-mermaid-swift, vendor/elk-swift   Mermaid dependencies
oracle/                     downright-oracle: the Swift reference for conformance
crates/                     the Rust port
crates/swift-text           Swift String/Character/CharacterSet/NSString semantics shared by
                            every crate, with Unicode tables generated from the Swift runtime
                            and checked for every scalar by the `unicode` suite
corpus/                     documents the conformance runner checks
```

## Conformance

`oracle/` is a small Swift package that links Downright's own `MarkdownCore` and `MarkdownRender`. It writes three outputs for a document:

- `parse` dumps the parsed document tree: every block, inline span, range, hash, and derived structure.
- `decorate` dumps every attribute run on the decorated `NSTextStorage`. Fonts, colors, and paragraph styles are compared bit for bit.
- `render` captures a PNG of the real `MarkdownContainerView` in an activated app, along with a dump of the layout fragments.

`upleft-oracle` takes the same arguments and writes the same formats. The runner compares the two outputs structurally and decodes the PNGs to compare pixels exactly. A render request can also be sent to the full app. Downright's `DOWNRIGHT_DEBUG_LAYOUT` and `DOWNRIGHT_DEBUG_CAPTURE` hooks capture the whole window, and Upleft implements the same hooks.

```sh
git submodule update --init
just oracle          # build downright-oracle
just conform         # run the corpus through both and compare
```

## Licensing

Upleft is MIT-licensed and keeps Downright's copyright notice. Ports of third-party code keep their upstream license:

- `upleft-markup` (from swift-markdown) is Apache-2.0.
- `upleft-elk` (from elk-swift) is EPL-2.0.
- `upleft-math` (from SwiftMath) and `upleft-mermaid` (from beautiful-mermaid-swift) are MIT.
