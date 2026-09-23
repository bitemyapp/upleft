# Porting beautiful-mermaid-swift to `upleft-mermaid`

`upleft-mermaid` ports beautiful-mermaid-swift (lukilabs, 1.0.4 @ `6a23a29`, MIT, Craft Docs) as Downright reaches it, plus Downright's `MermaidRendererBridge`. The Swift is the specification: layouts are compared as exact `f64`s and images pixel for pixel. Bugs are ported. The root `AGENTS.md` is binding.

## Status

Everything reachable from Downright's image path is ported and conformant on the whole corpus (see Conformance):

| Part | State |
|---|---|
| Parsers (flowchart, state, sequence, class, ER, xychart), `MermaidParser` | done |
| Layouts: sequence, xychart, and the ELK-backed flowchart/state (`src_layout`), class and ER | done; ELK is `upleft-elk` (feature `elk`, on by default) |
| CoreGraphics renderers (`Render/`) for every diagram type | done |
| `MermaidImageRenderer.prepare`, `PreparedDiagram`, `MermaidLayer.renderImage` | done |
| Downright `MermaidRendererBridge` | done (`downright::mermaid_renderer_bridge`), minus the image cache, which belongs to the fragment layer |

Not ported, because Downright cannot reach them: the ASCII renderers (`src_ascii_*`), the SVG renderers (`src_renderer`, `src_*_renderer`, `renderMermaidSVG`, `SVGHelpers` except `_hex`), `src_theme` (Shiki import), the unused `src_shape_clipping`, `ArrowRenderer`, the SwiftUI/UIKit views, and the public `MermaidRenderer` facade.

## ELK

`src_elk_instance::elk_layout_sync` calls `upleft_elk::bridge::elk::Elk::layout(&Value)` on one engine per thread, the way the Swift keeps one shared `ELK`. The ELK input is `serde_json::Value` built with the Swift literals' key order (`preserve_order`). `src_layout::elk_input_graph`, `src_class_layout::class_elk_input_graph` and `src_er_layout::er_elk_input_graph` expose it.

`tools/elk-capture.sh` runs beautiful-mermaid-swift's real layout with `elkLayoutSync` dynamically replaced and writes `corpus/mermaid-elk/<stem>.elkrec`: the trimmed source, every ELK input and answer, and the positioned graph. Each record comes from a single run. `src_elk_instance::replay` answers ELK calls from a record, and the uses are:

- the `mermaid-replay` suite, which checks that the port hands ELK byte-identical graphs and derives the identical positioned graph;
- the ELK-backed tests, which run on the records when built without `elk`;
- `UPLEFT_MERMAID_ELK_REPLAY=<dir>` for `upleft-oracle mermaid-layout|mermaid|mermaid-bench`, which isolates the non-ELK code.

Run `tools/elk-capture.sh` again after changing the corpus.

## Layout

One module per Swift file, snake_case: `Mermaid/src_parser.swift` → `mermaid::src_parser`, `Render/DiagramRenderer+Flow.swift` → `render::diagram_renderer_flow`, `ImageRenderer.swift` → `image_renderer`, `Theme.swift` → `theme`, `CrossPlatform.swift` → `cross_platform`. Two modules are not ports of one file:

- `swift`: Swift/Foundation behaviour the port depends on (probed against the macOS 26 runtime):
  - Regular expressions are ICU through `NSRegularExpression`, compiled once per thread (the Swift recompiles most of them per call). Group text follows `Range(nsRange, in:)`.
  - `count`, `dropFirst`, `dropLast`, `hasPrefix`, `hasSuffix`, `split(separator:)`, `Set<Character>.contains` and `String.contains` work on extended grapheme clusters under canonical equivalence. They delegate to `upleft-swift-text`. `"a\r\nb".contains("\n")` is false. `contains` is modelled as a native-string call; see docs/KNOWN-DIFFERENCES.md.
  - `replacingOccurrences(of:with:)` is NSString's search: it will not split a composed sequence, but it does match `\n` inside `\r\n`.
  - `components(separatedBy: "\n")` splits `\r\n`.
  - `Double(String)` is `strtod_l` with Swift's leading-whitespace and full-consumption rules. `"1e400"` is `inf`.
  - `Swift.min`/`max`, the variadic forms, `Sequence.min()`/`max()` and `sort(by:)` (Swift's merge sort) keep Swift's comparison order.
  - `String(format:)` goes through `snprintf`.
- `cg`: the CoreGraphics overlay calls. Path builders pass a pointer to the identity transform, as the Swift overlay does when `transform:` is defaulted, while `CGPath(rect:transform: nil)` passes NULL. `CGRect` accessors call the C getters.

`SDict`/`SSet` (in `src_types`) stand in for Swift `[String: V]`/`Set<String>`. Keys compare as NFC, and an update keeps the key that was stored first. Swift `Dictionary` iteration order is random per process. Wherever the Swift iterates one, the result was checked to be independent of the order, and the port uses first-insertion order. The one observable case is the order of sequence-diagram activations that are still open at the end: it is not visible in the pixels, and the dump sorts it.

## Public API (for the renderer port)

```rust
use upleft_mermaid::downright::mermaid_renderer_bridge as bridge;
bridge::image(source: &str, style_sheet: &StyleSheet) -> Option<MermaidImage>  // uncached body of image(source:styleSheet:)
bridge::cache_key(source, style_token) -> Option<MermaidCacheKey>             // MermaidCacheKey(source:styleToken:scale:)
bridge::theme(&StyleSheet) -> DiagramTheme                                    // theme(from:), also for HTML export
bridge::scale() -> f64                                                        // NSScreen.main?.backingScaleFactor ?? 2
MermaidImage { cg_image: CFRetained<CGImage>, size: CGSize }, .ns_image() -> Retained<NSImage>
```

`MermaidFragment` wraps `bridge::image` in `MarkdownFragmentImageCaches.mermaid` (48 entries, 24 MB, cost `source.utf8.count` for the key) and draws `ns_image()`. Lower level: `MermaidImageRenderer::new(theme, LayoutConfig::default()).prepare(src)`, `PreparedDiagram::render(ctx, bounds)`, `parser::parse`, `GraphLayout::layout`.

Threading: as in Swift, labels are drawn through `NSGraphicsContext`, using the caller's current context when one is set. `NSFontManager.shared` and `NSScreen.main` are reached without a main-thread check, also as in Swift. `MermaidFragment` renders lazily from `drawObject`. The renderer port has to decide where to call `bridge::image` so the main thread is not blocked (AGENTS.md). The images are pure functions of (source, style token, scale), so they can be computed ahead on a worker.

## Conformance

- The `downright-oracle` and `upleft-oracle` commands are `mermaid-parse`, `mermaid-layout` (parsed model, `DiagramTheme`, positioned graph and bounds), `mermaid` (the bridge's PNG, or a 1×1 magenta pixel for nil) and `mermaid-replay`. `MermaidModelDump.swift` is shared with the capture tool through a symlink.
- Corpus: `corpus/mermaid/*.mmd` holds 308 diagrams. `scripts/build-mermaid-corpus.py` regenerates the extracted `gen-*` and `bms-*` files; the `hand-*` and `bad-*` files are hand-written. Two xychart inputs make Downright trap. They live in `corpus/mermaid-traps/` and no suite reads them.
- Last full run (2026-09-23, real `upleft-elk`): `mermaid-parse` 308/308, `mermaid-layout` 1232/1232, `mermaid` 1232/1232 (4 theme/appearance variants), `mermaid-replay` 211/211. Six Swift runs of each of the 211 ELK-backed diagrams gave identical layouts, so the gates are exact.

## Benchmarks

`downright-oracle mermaid-bench <dir> <out.json>` and `upleft-oracle mermaid-bench …` report the best of `MERMAID_BENCH_ROUNDS`. `prepareMs` times `prepare(from:)`, which is parse and layout. `bridgeMs` times the whole uncached `MermaidRendererBridge.image` path: trim, prepare, draw, ink crop and `NSImage`. Both are release builds and use real ELK on each side. Results from 10 rounds on this machine (M-series):

| Set | Stage | Swift | Rust | Speed-up |
|---|---|---|---|---|
| whole corpus (306 diagrams, 293 images) | prepare | 175–182 ms | 21 ms | 8.4× |
| | bridge | 501 ms | 188 ms | 2.7× |
| ELK-backed (211) | prepare | 163 ms | 21 ms | 7.8× |
| | bridge | 347 ms | 119 ms | 2.9× |
| sequence + xychart (84) | prepare | 7.3 ms | 1.5–1.8 ms | 4.4× |
| | bridge | 135 ms | 81–85 ms | 1.6× |

Most of the remaining time is in CoreGraphics rasterisation and `NSAttributedString` drawing, which both sides call identically.
