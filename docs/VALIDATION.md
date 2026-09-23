# Validating the renderer

`upleft-render` and the crates beneath it (`upleft-core`, `upleft-markup`, `upleft-math`, `upleft-mermaid`, `upleft-elk`, `upleft-swift-text`) are the parts meant for reuse, Omperor included. This page covers how they are checked against Downright, what the checks cover, and how long they take.

## Principles

- **Differential.** Every check runs the same input through `downright-oracle` and `upleft-oracle`. The Swift side is Downright's own code, rebranded (see AGENTS.md, "App identity"). Dumps are compared structurally, with doubles compared bit for bit. Images are compared pixel for pixel.
- **Headless.** Window checks use a borderless window placed outside every screen and never activate the app. `cacheDisplay` records it. Nothing appears on screen and focus never moves.
- **Parallel and low priority.** Headless captures share nothing on screen, so `conform` runs them on half the cores at nice 10. Only the opt-in `--capture screen` mode is serialised by the machine-wide lock.
- **Cached.** Swift results are cached by oracle binary, input and flags, so after the first run only the Rust side is re-rendered.

## What is checked

| Suite | Cases | What it proves |
|---|---:|---|
| `markup`, `parse`, `parse-io` | 909 + 14 | swift-markdown's tree and Downright's parse, including ranges, hashes and derived structures |
| `decorate`, `incremental`, `displaymap`, `clipboard` | 5454, 2727, 2727, 909 | every attribute run in every mode and theme, incremental edit commits, the source-to-display map, clipboard HTML |
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
