# Validating the renderer

`upleft-render` and the crates beneath it (`upleft-core`, `upleft-markup`, `upleft-math`, `upleft-mermaid`, `upleft-elk`, `upleft-swift-text`) are the parts meant for reuse, Omperor included. This page covers how they are checked against Downright, what the checks cover, and how long they take.

Upleft parses Markdown with pulldown-cmark, not the cmark-gfm C library Downright uses. The adapter in `upleft-markup` rebuilds the tree and source ranges swift-markdown gets from cmark-gfm, so every check below still compares against Downright's real parse. Where the two parsers genuinely disagree the suites show it; "Parser differential" below says how to tell those cases apart.

## Principles

- **Differential.** Every check runs the same input through `downright-oracle` and `upleft-oracle`. The Swift side is Downright's own code, rebranded (see AGENTS.md, "App identity"). Dumps are compared structurally, with doubles compared bit for bit. Images are compared pixel for pixel.
- **Headless.** Window checks use a borderless window placed outside every screen and never activate the app. `cacheDisplay` records it. Nothing appears on screen and focus never moves.
- **Parallel and low priority.** Headless captures share nothing on screen, so `conform` runs them on half the cores at nice 10. Only the opt-in `--capture screen` mode is serialised by the machine-wide lock.
- **Cached.** Swift results are cached by oracle binary, input and flags, so after the first run only the Rust side is re-rendered. The cache does not key on the OS: after a macOS update, cached Swift captures can differ from fresh ones by one colour level in antialiased text. On macOS 26.6.2 that made `render-state` 62/100 and `render` on `generated/docs/` 107/116 against the cache, identically on `main`; with fresh Swift captures (`--no-cache`) both are 100/100 and 116/116. Rerun with `--no-cache` after an OS update.

## What is checked

| Suite | Cases | What it proves |
|---|---:|---|
| `markup`, `parse`, `parse-io` | 915 + 14 | the adapter's tree against swift-markdown's, and Downright's parse, including ranges, hashes and derived structures |
| `decorate`, `incremental`, `displaymap`, `clipboard` | 5490, 2745, 2745, 915 | every attribute run in every mode and theme, incremental edit commits, the source-to-display map, clipboard HTML |
| `highlight`, `stylesheet`, `vscode-theme` | 909, 12, 19 | syntax runs, every resolved font, colour and metric, theme import |
| `math-image`, `math-tree` | 15420 each | formula bitmaps and display trees, 6 themes × light and dark |
| `elk`, `mermaid-*` | 449, 308 + 1232 × 2 + 211 | diagram layout and bitmaps |
| `render` | 3636 | the real text view, every corpus document, in 4 variants: pixels plus the layout of every fragment |
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
- **streamed appends**, both at line boundaries and in token-sized pieces that leave fences, tables, math and diagrams open mid-stream. This is how a chat transcript arrives in Omperor. Every append goes through reparse, `ASTDiff`, `update(document:dirty:)`, and a frame.
- in-place edits.

Every scenario is one capture compared against Downright pixel for pixel.

## Parser differential

`crates/markup/examples/markup_diff.rs` parses each input with the pulldown-cmark adapter and with the cmark-gfm converter it replaced (`parser::cmark_oracle`, compiled only for tests and under the `cmark-oracle` feature), dumps both trees in the `markup` suite's format and counts differences by category. It needs no Swift build and runs in seconds.

```sh
cargo run --release -p upleft-markup --features cmark-oracle --example markup_diff -- \
    [--corpus] [--spec] [--incremental] [--mutations N] [--random N] [--minimize] [FILE...]
```

`--incremental` checks the texts the `incremental` suite parses after each of its eight edits; `--mutations` and `--random` generate seeded documents; `--minimize` shrinks every differing input to a minimal repro. `cargo test -p upleft-markup` runs the same comparison on every quirk the tool has found and on the spec examples.

Baseline (2026-09-23):

| Inputs | Identical to cmark-gfm |
|---|---:|
| corpus documents | 251/251 |
| cmark spec examples | 744/744 |
| texts the `incremental` suite parses | 7923/7960 |
| mutated documents (seeded) | 19808/20000 |
| random documents (seeded) | 19786/20000 |

All 443 differing inputs minimize to one of the genuine parser differences in docs/KNOWN-DIFFERENCES.md ("Markdown parser"). That fixes what the suites should report:

- `markup` and `parse`: 915/915. The corpus contains none of the residual constructs. A new corpus document that does will fail both; check it against the ledger before treating it as a bug.
- `incremental`: 2718/2745. The 27 failures are nine documents (in three modes each) whose edited texts hit a residual difference: `MarkdownRenderTests__LayoutFillerTests-001.md` and `quicklook-thumbnail/tasks-half.md` (a task box with no text, then a less indented line), `spec/regression-0006.md`, `-0009.md` and `-0014.md` (an HTML tag line right after a list item), `spec/regression-0011.md` (single-tilde strikethrough inside a word), `spec/spec-0647.md` and `spec-0648.md` (a declaration with a lowercase name), and `workspace/export/fragments.md` (a link destination with an unbalanced parenthesis and a space).
- Every other suite: 100%.

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
