# Upleft's day-to-day tasks. `just` alone lists them.

scratch := justfile_directory() / "target"
# The SDK every binary is stamped with; see docs/BUILD-VERSION.md.
sdk_version := `xcrun --sdk macosx --show-sdk-version`

default:
    @just --list --unsorted

# Build downright-oracle, the Swift reference, from vendor/downright.
# `-enable-testing` lets the oracle `@testable import` Downright's modules to
# dump internal state; it changes symbol visibility, not behaviour.
oracle:
    cd oracle && swift build -c release -Xswiftc -enable-testing --scratch-path {{scratch}}/oracle
    just stamp {{scratch}}/oracle/release/downright-oracle

# Build downright-app-oracle, the Swift reference for the app-layer suites
# (oracle/app: Downright's own module sources compiled as libraries, see its
# Package.swift). Separate from `just oracle` so the core suites never need
# the whole app or Sparkle.
app-oracle:
    swift build --package-path oracle/app -c release -Xswiftc -enable-testing --scratch-path {{scratch}}/app-oracle
    just stamp {{scratch}}/app-oracle/release/downright-app-oracle

# Build Downright's real `down` command-line tool from the submodule, stamped,
# for the `down-cli` suite.
downright-cli:
    cd vendor/downright && swift build -c release --scratch-path ../../target/downright-cli --product down
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
drbench:
    cd vendor/downright && swift build -c release --scratch-path ../../target/drbench --product drbench
    just stamp {{scratch}}/drbench/release/drbench

# Compare drbench (Swift) with upleft-bench (Rust) stage by stage; fails on any
# stage slower than Swift beyond run-to-run noise.
bench *args: drbench
    cargo build --release -p upleft-bench
    python3 scripts/bench-compare.py {{args}}

# Build the original Downright.app from the submodule (for window-level conformance).
downright-app:
    cd vendor/downright && SCRATCH=../../target/downright-app Scripts/bundle-app.sh
    just stamp {{scratch}}/downright-app/bundle/Downright.app/Contents/MacOS/Downright
    codesign --force --sign - {{scratch}}/downright-app/bundle/Downright.app

# Run the Rust test suite.
test:
    cargo test --workspace

# Regenerate corpus/generated from the pinned submodules.
corpus:
    python3 scripts/build-corpus.py

# Compare Upleft with Downright on the corpus. Pass suite filters through, e.g. `just conform --suite parse`.
conform *args: corpus
    cargo build --release -p upleft-conformance -p upleft-cli
    target/release/conform {{args}}
