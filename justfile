# Upleft's day-to-day tasks. `just` alone lists them.

scratch := justfile_directory() / "target"
# The SDK every binary is stamped with; see docs/BUILD-VERSION.md.
sdk_version := `xcrun --sdk macosx --show-sdk-version`

default:
    @just --list --unsorted

# Build downright-oracle, the Swift reference, from the rebranded copy of
# vendor/downright (see scripts/rebrand.py and AGENTS.md, "App identity").
# `-enable-testing` lets the oracle `@testable import` Downright's modules to
# dump internal state; it changes symbol visibility, not behaviour.
oracle: rebrand
    cd oracle && swift build -c release -Xswiftc -enable-testing --scratch-path {{scratch}}/oracle
    just stamp {{scratch}}/oracle/release/downright-oracle

# oracle/app compiles Downright's own module sources as libraries (see its
# Package.swift). It is separate from `just oracle` so the core suites never
# need the whole app or Sparkle.
# Build downright-app-oracle, the Swift reference for the app-layer suites.
app-oracle: rebrand
    swift build --package-path oracle/app -c release -Xswiftc -enable-testing --scratch-path {{scratch}}/app-oracle
    just stamp {{scratch}}/app-oracle/release/downright-app-oracle

# Build Downright's real `down` from the submodule, stamped, for `down-cli`.
downright-cli: rebrand
    cd target/rebranded/downright && swift build -c release --scratch-path ../../downright-cli --product down
    just stamp {{scratch}}/downright-cli/release/down

# Stamp a Swift binary with the canonical LC_BUILD_VERSION (minos 14.0, the
# installed SDK) and re-sign it ad hoc. SwiftPM records sdk 14.0; an Xcode
# build of Downright records the real SDK, and AppKit keys behaviour off it.
stamp binary:
    vtool -set-build-version macos 14.0 {{sdk_version}} -replace -output "{{binary}}.stamped" "{{binary}}"
    mv "{{binary}}.stamped" "{{binary}}"
    codesign --force --sign - "{{binary}}"

# Build the instrumented elk-swift copy that settles the graphs on which
# elk-swift itself is nondeterministic (see crates/elk/PORTING.md).
elklab:
    crates/elk/tools/elklab.sh

# Build Downright's own benchmark (drbench) from the submodule, stamped like
# every other reference binary.
drbench: rebrand
    cd target/rebranded/downright && swift build -c release --scratch-path ../../drbench --product drbench
    just stamp {{scratch}}/drbench/release/drbench

# Compare drbench (Swift) with upleft-bench (Rust) stage by stage; fails on any
# stage slower than Swift beyond run-to-run noise.
bench *args: drbench
    cargo build --release -p upleft-bench
    python3 scripts/bench-compare.py {{args}}

# Runs downright-app-oracle (Swift) and upleft-oracle (Rust) stage by stage
# and fails on any stage slower than Swift beyond run-to-run noise.
# Compare the app layer's benchmarks: HTML export, workspace index, find.
app-bench *args: app-oracle corpus
    cargo build --release -p upleft-conformance
    python3 scripts/app-bench-compare.py {{args}}

# Build the original Downright.app from the submodule (for window-level conformance).
downright-app: rebrand
    cd target/rebranded/downright && SCRATCH=../../downright-app Scripts/bundle-app.sh
    just stamp {{scratch}}/downright-app/bundle/Upleft.app/Contents/MacOS/Upleft
    codesign --force --sign - {{scratch}}/downright-app/bundle/Upleft.app

# Run the Rust test suite.
test:
    cargo test --workspace

# Build target/rebranded/downright: Downright with the Upleft identity applied
# to visible strings only. Every Swift reference builds from it.
rebrand:
    python3 scripts/rebrand.py copy

# Regenerate corpus/generated from the pinned submodules.
corpus:
    python3 scripts/build-corpus.py

# Compare Upleft with Downright on the corpus. Pass suite filters through, e.g. `just conform --suite parse`.
conform *args: corpus
    cargo build --release -p upleft-conformance -p upleft-cli
    target/release/conform {{args}}

# Assemble target/upleft-app/Upleft.app (Scripts/bundle-app.sh's layout with the
# Upleft identity: Info.plist from the rebranded template, `down`, the math
# fonts, Welcome.md, Sparkle.framework 2.9.6, the Spotlight importer, the Quick
# Look extensions, ad-hoc signature). Builds it; never registers or launches it.
upleft-app: rebrand
    scripts/bundle-upleft-app.sh

# Rebuild and re-embed only the Quick Look extensions (DownrightQL.appex,
# DownrightThumb.appex) in an existing Upleft.app; re-signs and verifies it.
# Never registers them with pluginkit or launches them.
upleft-quicklook app="target/upleft-app/Upleft.app": rebrand
    scripts/bundle-upleft-quicklook.sh APP={{app}}

# Compare the panels' build, layout and draw timings (corpus/panel-bench)
# between downright-app-oracle and upleft-oracle; fails on a slower stage.
panel-bench *args: corpus
    cargo build --release -p upleft-conformance
    python3 scripts/panel-bench-compare.py {{args}}

# Compare the document window's timings (open to first frame, mode switch)
# between downright-app-oracle and upleft-oracle; fails on a slower stage.
# Windows are off-screen and never activated.
app-window-bench *args: corpus
    cargo build --release -p upleft-conformance
    python3 scripts/app-window-bench-compare.py {{args}}
