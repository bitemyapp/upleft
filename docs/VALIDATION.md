# Validating the renderer

`upleft-render` and the crates beneath it (`upleft-core`, `upleft-markup`, `upleft-math`, `upleft-mermaid`, `upleft-elk`, `upleft-swift-text`) are the parts meant for reuse in other applications. This page covers how they are checked against Downright, what the checks cover, and how long they take.

Upleft parses Markdown with its fork of pulldown-cmark, not the cmark-gfm C library Downright uses. The fork's `ENABLE_CMARK_GFM_COMPAT` option parses as cmark-gfm does, and the adapter in `upleft-markup` rebuilds the tree and source ranges swift-markdown gets from cmark-gfm, so every check below still compares against Downright's real parse. "Parser differential" below says how the parser alone is checked.

## Principles

- **Differential.** Every check runs the same input through `downright-oracle` and `upleft-oracle`. The Swift side is Downright's own code, rebranded (see AGENTS.md, "App identity"). Dumps are compared structurally, with doubles compared bit for bit. Images are compared pixel for pixel.
- **Headless.** Window checks use a borderless window placed outside every screen and never activate the app. `cacheDisplay` records it. Nothing appears on screen and focus never moves.
- **Parallel and low priority.** Headless captures share nothing on screen, so `conform` runs them on half the cores at nice 10. Only the opt-in `--capture screen` mode is serialised by the machine-wide lock.
- **Cached.** Swift results are cached by oracle binary, macOS build, display setup, input and flags, so after the first run only the Rust side is re-rendered. Each part of the key once let stale captures through:
  - **macOS build:** an OS update can move antialiased text by one colour level. After the update to 26.6.2, stale captures made `render-state` report 62/100; with fresh captures it is 100/100.
  - **Display setup** (each screen's size and backing scale): an off-screen capture takes its backing scale from the screens. Captures cached while a 1× display was the main one failed the same 38 cases once a 2× display was.

## What is checked

| Suite | Cases | What it proves |
|---|---:|---|
| `markup`, `parse`, `parse-io` | 915 + 14 | the adapter's tree against swift-markdown's, and Downright's parse, including ranges, hashes and derived structures |
| `decorate`, `incremental`, `displaymap`, `clipboard` | 5490, 2745, 2745, 915 | every attribute run in every mode and theme, incremental edit commits, the source-to-display map, clipboard HTML |
| `highlight`, `stylesheet`, `vscode-theme` | 909, 12, 19 | syntax runs, every resolved font, colour and metric, theme import |
| `math-image`, `math-tree` | 15420 each | formula bitmaps and display trees, 6 themes × light and dark |
| `elk`, `mermaid-*` | 449, 308 + 1232 × 2 + 211 | diagram layout and bitmaps |
| `render` | 3660 | the real text view, every corpus document, in 4 variants: pixels plus the layout of every fragment |
| `render-dark-themes`, `render-images`, `render-density`, `density-*` | 100, 28, 1126, 1830 + 240 | themed fragments, loaded local images, the document map |
| `render-state` | 100 | what the rest cannot see (below) |

`render-state` (`corpus/render-state/*.json`, written by `scripts/build-render-state-corpus.py`) drives the view through its public surface before capturing. It covers:

- all six themes in light and dark;
- Read mode;
- scroll positions below the fold, at the middle and the end of long documents;
- narrow and wide measures;
- render configuration: invisibles, reflow, typographic substitution, reveal policy, code-collapse threshold;
- collapsed and expanded code;
- search hits and the current hit, speech highlight, single and multiple selections;
- change marks (inserted, modified with word ranges, deletion ghosts, visited);
- source focus;
- folding and all four structural zoom levels;
- motion with Reduce Motion off;
- **streamed appends**, both at line boundaries and in token-sized pieces that leave fences, tables, math and diagrams open mid-stream. This is how a chat transcript arrives from a language model. Every append goes through reparse, `ASTDiff`, `update(document:dirty:)`, and a frame.
- in-place edits.

Every scenario is one capture compared against Downright pixel for pixel.

## Parser differential

`crates/markup/examples/markup_diff.rs` parses each input with the pulldown-cmark adapter and with the cmark-gfm converter it replaced (`parser::cmark_oracle`, compiled only for tests and under the `cmark-oracle` feature), dumps both trees in the `markup` suite's format and counts differences by category. It needs no Swift build and runs in seconds.

```sh
cargo run --release -p upleft-markup --features cmark-oracle --example markup_diff -- \
    [--corpus] [--spec] [--incremental] [--mutations N] [--random N] [--lines N] [--seed N] \
    [--minimize] [--dump] [--text MARKDOWN] [FILE...]
```

`--incremental` applies the `incremental` suite's eight edits to every corpus document (the suite itself skips `workspace/`, `spotlight/` and `quicklook-thumbnail/`) and checks each edited text; `--mutations` and `--random` generate documents, and `--lines` builds documents line by line behind random container prefixes (list markers, quotes, tabs); `--seed` changes the generators' seed; `--text` adds an input given on the command line; `--dump` prints both trees of every differing input; `--minimize` shrinks every differing input to a minimal repro. With no source flags it checks the corpus, the spec examples, the edited texts and 2,000 mutated and 2,000 random documents. `cargo test -p upleft-markup` runs the same comparison on every quirk the tool has found, on patterns common in chat transcripts, and on the spec examples.

Baseline (2026-09-24):

| Inputs | Identical to cmark-gfm |
|---|---:|
| corpus documents | 251/251 |
| cmark spec examples | 744/744 |
| corpus documents after each `incremental` edit | 7960/7960 |
| mutated documents, seeds 1–5 | 1,000,000/1,000,000 |
| random documents, seeds 1–5 | 1,000,000/1,000,000 |
| line-built documents, seeds 1–5 | 2,000,000/2,000,000 |

Seeds 21–32 (line-built, 400,000 each) and 41–46 (200,000 mutated and 200,000 random each) also minimize to nothing. The parser has no known difference from cmark-gfm (docs/KNOWN-DIFFERENCES.md, "Markdown parser"), so every suite that parses should pass:

- `markup` and `parse`: 915/915.
- `incremental`: 2745/2745.
- Every other suite: 100%.

A difference the tool or a suite finds is a bug in the fork's `ENABLE_CMARK_GFM_COMPAT` option or in the adapter. Minimize it, add it to the regression tests, and fix it.

## Selector audit

A Swift override that the port registers under the wrong Objective-C selector compiles and runs, but AppKit never calls it. The bug found this way: `drawBackground(in:)` is `drawViewBackgroundInRect:`, not `drawBackgroundInRect:`. `just selector-audit` renders a few scenarios with `UPLEFT_SELECTOR_AUDIT=1`. It lists every method a class in the binary registers that neither its superclass nor an adopted protocol declares. The list must contain only methods Downright itself adds (for example `pasteAsMarkdown:`). `scripts/check-protocol-selectors.py` checks protocol methods statically.

## Running

```sh
just corpus                  # regenerate corpus/generated from the submodules
just render-state-corpus     # regenerate corpus/render-state
just conform --suite render-state
just conform --suite render --suite render-dark-themes --suite render-images
just selector-audit
```

On an Apple Silicon MacBook Pro, `render-state` takes about 25 seconds. The full `render` suite takes about 15 minutes with an empty Swift cache, and about half that once Swift results are cached. Run a suite when the code it covers changes; there is no need to re-run everything after every edit.
