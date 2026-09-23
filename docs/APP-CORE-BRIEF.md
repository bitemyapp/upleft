# App-core porting brief

Shared instructions for the agents porting Downright's app layer and command-line tools (branch `port/app-core` and its sub-branches). Read this, then `AGENTS.md`, `README.md`, `docs/BUILD-VERSION.md` and `docs/KNOWN-DIFFERENCES.md`. All of them are binding.

## Contract

Upleft must behave exactly like the Swift original: byte-identical output wherever Downright writes bytes (files, stdout, stderr, JSON, HTML), and it must be at least as fast. Port, don't redesign:

- One Rust module per Swift file (snake_case), with the same algorithms, order of operations and constants.
- Use the same Foundation and AppKit calls where behaviour depends on them: file coordination, FSEvents, UserDefaults keys and value types, JSON output formatting, date formatting and sorting.
- Reproduce bugs as well. Record every unavoidable difference in `docs/KNOWN-DIFFERENCES.md`: append rows at the end of the Open table.
- Probe with small Swift programs whenever you are unsure. `swiftc -O probe.swift` in the scratchpad works.
- Never block the AppKit main thread.
- Never modify `vendor/`.

## Layout already in place (commit 2f20950)

- `crates/foundation` (`upleft-foundation`) holds support shared by every crate. Use it and don't duplicate it. You may add functions to it, but only by adding: never change existing behaviour without saying so in your report.
  - `json_encoder`: Swift `JSONEncoder` output. `JsonValue` trees with members in `CodingKeys` order, and `OutputFormatting`.
  - `json_serialization`: `NSJSONSerialization` through objc2, with `AnyJson` trees.
  - `url::FileUrl`: Swift `URL` semantics for file URLs.
  - `date::Date`: seconds since the reference date, `.iso8601`.
- `crates/cli` (`upleft-cli`) mirrors `Sources/drdownright` and `Sources/down` (binary `down` at `src/bin/down/main.rs`). `crates/spotlight-metadata` mirrors `Sources/DownrightSpotlightMetadata`.
- `crates/app` (`upleft-app`) mirrors `Sources/DownrightApp/<Folder>/<File>.swift` as `src/<folder>/<file>.rs`. Every module is already declared, as a stub file. Fill in your own stubs and leave `lib.rs` and the other agents' files alone. `support::app_paths` and `app::document_types` are already ported.
- Conformance (see `README.md` for the runner):
  - Swift side: `oracle/app/`, a separate package whose `Sources/<Module>` directories are symlinks to the submodule's sources, compiled unchanged, so `@testable import DownrightApp` works. Build it with `just app-oracle`. Each suite has a stub, `oracle/app/Sources/downright-app-oracle/<Name>Dump.swift`, returning a `JSON` (same helper as the core oracle). Replace your stub. `main.swift` already dispatches every app command.
  - Rust side: `crates/conformance/src/dump/<name>.rs`, a stub returning `NotPorted`. Replace it; `request.flags` holds the raw flags.
  - `conformance/suites.json` already has an entry for each app suite (`"oracle": "app"`). Adjust your own entry in place and don't reorder entries. Inputs live under `corpus/<suite>/` and are committed. `corpus/workspace/` is excluded from every suite that doesn't name it in `only`.
  - Run with `just app-oracle && cargo build --release -p upleft-conformance -p upleft-cli && target/release/conform --suite NAME`, or `just conform --suite NAME`, which also regenerates `corpus/generated`. Swift results are cached per input, oracle binary and flags. Pass `--no-cache` after changing a Swift dump without rebuilding the oracle.
  - Dump byte-exact text (HTML, files, stdout) as JSON arrays of lines, split on `"\n"` so nothing is lost, so that a difference names the line. Dump binary content as hex.
  - Both oracles run with `HOME` and `CFFIXED_USER_HOME` set to `target/conform-home`.

## Foundation facts already established (Swift 6.4, macOS 26)

- **`JSONEncoder` key order.** Without `.sortedKeys`, key order changes from process to process, because swift-foundation keeps keyed containers in a hash table. No byte-exact comparison is possible for such files. Compare them as parsed JSON and write members in `CodingKeys` order. With `.sortedKeys`, keys are ordered by Swift `String <`. `JSONSerialization`'s `.sortedKeys` instead uses `NSString` comparison, which is why it goes through objc2.
- **Home directory.** `NSHomeDirectory()`, `FileManager.urls(for:in:)` and `~` expansion ignore `HOME` and honour `CFFIXED_USER_HOME`. Any sandbox must set `CFFIXED_USER_HOME`, and `HOME` too. Never read or write the real home: the machine has a real `~/.claude/settings.json` and real Downright state.
- **`URL(fileURLWithPath:)`.** It expands `~` (NSURL doesn't) and resolves relative paths against the current directory, removing dot segments. It converts the path to its file-system representation, so `é` becomes `e` + U+0301: `.path`, hashes of paths and JSON all see decomposed text. It stats the path to decide whether the URL is a directory. `appendingPathComponent` stats too. Use `FileUrl`. If you find a case where it disagrees with Swift, fix it in `url.rs` with a test quoting the probe.
- **`Date` in `.iso8601`.** Whole seconds, floored, in UTC with a `Z` suffix.
- **Swift `String` semantics.** `upleft-swift-text` covers graphemes, `lowercased`, `trimmingCharacters`, `components(separatedBy:)`, `<` (`str_less`), `==` and `contains`. Use it for every string operation whose Swift semantics matter. Downright's positions are UTF-16 offsets (`NSRange`).
- **Clocks.** Where Swift reads the clock (`Date()`) and the code has no injection point, a conformance dump must normalise only the timestamp fields, and must say which fields in the dump's doc comment. Where the Swift API takes a `now:` or clock parameter, inject the same value on both sides.
- **`UserDefaults`.** In dumps and tests, use a unique `UserDefaults(suiteName:)` and remove its persistent domain afterwards. Never use `.standard`.

## Working rules

- Commit often on your branch with `git -c user.name="Chris Allen" -c user.email="cma@bitemyapp.com" commit`. End every message with a blank line and `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Don't push, and don't merge into `port/app-core` or `main`. The app-core coordinator merges your branch.
- Port the matching Swift tests as Rust integration tests in `crates/<crate>/tests/<swift_test_file_in_snake_case>.rs`. Keep each test's name and assertions, and skip only what needs a window. List the skipped tests, with the reason, in your report.
- `cargo test -p <your crate>` must pass. Don't break `cargo build --workspace`.
- Your `target/` was cloned from the coordinator's build, so the first build is incremental.
- If your context runs low, commit, write what is done, what is left and how to continue in `crates/<crate>/PORTING-<area>.md`, and report.

## Report

Keep the report short and factual:

- the file mapping (Swift file → Rust module)
- conformance numbers per suite (cases, pass, fail, error)
- tests ported (count, and the skipped ones with the reason)
- benchmarks, if any
- known differences you added
- commits
- gaps

Label anything unverified as unverified.
