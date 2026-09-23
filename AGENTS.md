# Upleft agent guidelines

Upleft is a faithful Rust port of Downright (`vendor/downright`). The Swift original is the specification. When the two disagree, Upleft is wrong.

## The contract

- **Output identical to Downright's.** Parse dumps, decoration dumps, layout, and pixels must match Downright's for every corpus document, theme, appearance, and mode. "Close" is a failure. Pixel comparison is exact.
- **Same speed or faster.** Each Downright benchmark stage (`Sources/drbench`) has a Rust counterpart, `upleft-bench`, which runs the same corpus and the same stages. A slower stage is a regression.
- **Port, don't redesign.** Keep Downright's module and file structure: one Rust module per Swift file, with the same names in snake_case. Keep the same algorithms, the same order of operations, and the same constants. Idiomatic Rust is welcome at the data-structure level. Behaviour, including floating-point evaluation order in layout and drawing code, stays the same as the Swift.
- **Never modify `vendor/`.** Submodules are pinned references. If a port needs a Swift fact, read it there.

## Porting rules

- **Positions.** Downright's positions are UTF-16 offsets (`NSRange`). Keep them as UTF-16 offsets. Convert to UTF-8 only at the cmark boundary, exactly as swift-markdown and `SourcePositions.swift` do.
- **Swift `String` semantics.**
  - `count`, `Character`, and `String.Index` walk extended grapheme clusters. Where Downright uses them, the port must too.
  - `lowercased()`, `trimmingCharacters(in:)`, `components(separatedBy:)`, and `NSString`/`CharacterSet` methods each have exact Unicode behaviour. Match it, calling Foundation through objc2 when needed.
- **Numbers.**
  - `CGFloat` is `f64`.
  - Swift's `rounded()` is `f64::round` (ties away from zero).
  - `Int(x)` truncates.
  - `&+` and `&*` are `wrapping_add` and `wrapping_mul`.
  - Do not reorder floating-point expressions.
- **AppKit and TextKit 2.** Call them through objc2 (`objc2-app-kit`, `objc2-foundation`, `objc2-core-graphics`, `objc2-core-text`).
  - Subclasses such as `NSTextView`, `NSTextLayoutFragment`, `NSTextContentStorage`, and delegates are `define_class!` types.
  - An Objective-C class name must equal the Swift class's unqualified name (`TableFragment`, `MarkdownTextView`), so layout dumps compare.
  - Make the same framework calls, with the same arguments, in the same order. That is what makes pixels match.
- **Main thread.** Never block the AppKit main thread with parsing, I/O, or subprocesses. Follow Downright's threading exactly (see `Docs/ARCHITECTURE.md` in the submodule), and be at least as strict where it is lax.

## Gates

- `cargo test --workspace` must pass.
- `just conform` must report zero differences for every layer that is marked ported.
- A layer is not marked ported in `README.md` until its gate is green on the whole corpus.
- Report results for each layer, and label anything unverified as unverified.
