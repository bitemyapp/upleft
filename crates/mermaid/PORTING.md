# Porting beautiful-mermaid-swift to `upleft-mermaid`

`upleft-mermaid` ports beautiful-mermaid-swift (lukilabs, 1.0.4 @ `6a23a29`, MIT, Craft Docs) as Downright reaches it, plus Downright's `MermaidRendererBridge`. The Swift is the specification: layouts are compared as exact `f64`s and images pixel for pixel. Bugs are ported. The root `AGENTS.md` is binding.

## Status

| Part | State |
|---|---|
| Parsers (flowchart, state, sequence, class, ER, xychart), `MermaidParser` | done; `mermaid-parse` 308/308 |
| Sequence and xychart layouts | done; `mermaid-layout` passes every non-ELK case |
| Flowchart/state, class and ER layouts (`src_layout`, `src_class_layout`, `src_er_layout`) | ported, not verified: they call ELK through `src_elk_instance`, and `upleft-elk` is not linked yet |
| CoreGraphics renderers (`Render/`) | done for sequence and xychart (pixel-identical); flow, class and ER renderers are ported but unverified until ELK lands |
| `MermaidImageRenderer.prepare`, `PreparedDiagram`, `MermaidLayer.renderImage` | done |
| Downright `MermaidRendererBridge` | done (`downright::mermaid_renderer_bridge`), minus the image cache, which belongs to the fragment layer |

Not ported, because Downright cannot reach them: the ASCII renderers (`src_ascii_*`), the SVG renderers (`src_renderer`, `src_*_renderer`, `renderMermaidSVG`, `SVGHelpers` except `_hex`), `src_theme` (Shiki import), the unused `src_shape_clipping`, `ArrowRenderer`, the SwiftUI/UIKit views, and the public `MermaidRenderer` facade.

## Enabling ELK

1. When `upleft-elk` is merged and its conformance is green, add `upleft-elk = { workspace = true, optional = true }` to this crate and change the feature to `elk = ["dep:upleft-elk"]`; make it a default feature (or enable it from `upleft-conformance`).
2. `src_elk_instance::engine` (the `#[cfg(feature = "elk")]` half) calls `upleft_elk::bridge::elk::Elk::layout(&Value)` once per thread-local engine, the way the Swift keeps one shared `ELK`.
3. The ELK input is `serde_json::Value` built with the Swift literals' key order (`preserve_order`). `src_layout::elk_input_graph`, `src_class_layout::class_elk_input_graph` and `src_er_layout::er_elk_input_graph` expose it for debugging.
4. Run `just conform --suite mermaid-layout --suite mermaid`. The 844 "not ported" cases become real comparisons. The ignored tests in `tests/` run with `cargo test -p upleft-mermaid --features elk`.

## Layout

One module per Swift file, snake_case: `Mermaid/src_parser.swift` → `mermaid::src_parser`, `Render/DiagramRenderer+Flow.swift` → `render::diagram_renderer_flow`, `ImageRenderer.swift` → `image_renderer`, `Theme.swift` → `theme`, `CrossPlatform.swift` → `cross_platform`. Two modules are not ports of one file:

- `swift`: Swift/Foundation behaviour the port depends on (probed against the macOS 26 runtime):
  - Regular expressions are ICU through `NSRegularExpression`, compiled once per thread (the Swift recompiles most of them per call). Group text follows `Range(nsRange, in:)`.
  - `count`, `dropFirst`, `dropLast`, `hasPrefix`, `hasSuffix`, `split(separator:)`, `Set<Character>.contains` and `String.contains` work on extended grapheme clusters under canonical equivalence. `"a\r\nb".contains("\n")` is false.
  - `replacingOccurrences(of:with:)` is NSString's search: it will not split a composed sequence, but it does match `\n` inside `\r\n`.
  - `components(separatedBy: "\n")` splits `\r\n`.
  - `Double(String)` is `strtod_l` with Swift's leading-whitespace and full-consumption rules. `"1e400"` is `inf`.
  - `Swift.min`/`max`, the variadic forms, `Sequence.min()`/`max()` and `sort(by:)` (Swift's merge sort) keep Swift's comparison order.
  - `String(format:)` goes through `snprintf`.
- `cg`: the CoreGraphics overlay calls. Path builders pass a pointer to the identity transform, as the Swift overlay does when `transform:` is defaulted, while `CGPath(rect:transform: nil)` passes NULL. `CGRect` accessors call the C getters.

`SDict`/`SSet` (in `src_types`) stand in for Swift `[String: V]`/`Set<String>`. Keys compare as NFC, and an update keeps the key that was stored first.

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

Threading: like the Swift, labels draw through `NSGraphicsContext` and use the current one if a caller has set it. `NSFontManager.shared` and `NSScreen.main` are reached without a main-thread check, as in Swift.

## Conformance

- `downright-oracle`/`upleft-oracle` `mermaid-parse`, `mermaid-layout` (parsed model, `DiagramTheme`, positioned graph, bounds) and `mermaid` (the bridge's PNG, or a 1×1 magenta pixel for nil). The Rust side exits 3 ("not ported") where a layout needs ELK.
- Corpus: `corpus/mermaid/*.mmd` (310 files, including the 2 `.mmdtrap` inputs in `corpus/mermaid-traps/`). `scripts/build-mermaid-corpus.py` regenerates the extracted `gen-*`/`bms-*` files. `hand-*`/`bad-*` files are hand-written.
- Last full run (2026-09-23, `--features` off): `mermaid-parse` 308/308; `mermaid-layout` 388 pass, 844 not ported, 0 fail; `mermaid` 388 pass, 844 not ported, 0 fail.

## Benchmarks

`downright-oracle mermaid-bench <dir> <out.json>` and `upleft-oracle mermaid-bench …` time `prepare(from:)` and the whole uncached bridge path (best of `MERMAID_BENCH_ROUNDS`). On the 84 sequence and xychart diagrams of the corpus, on an M-series Mac with 10 rounds:

| Stage | Swift (release) | Rust (release) |
|---|---|---|
| prepare | 7.3 ms | 1.5–1.8 ms |
| bridge (prepare + draw + crop + NSImage) | 135 ms | 81–85 ms |
