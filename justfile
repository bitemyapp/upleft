# Upleft's day-to-day tasks. `just` alone lists them.

scratch := justfile_directory() / "target"

default:
    @just --list --unsorted

# Build downright-oracle, the Swift reference, from vendor/downright.
# `-enable-testing` lets the oracle `@testable import` Downright's modules to
# dump internal state; it changes symbol visibility, not behaviour.
oracle:
    cd oracle && swift build -c release -Xswiftc -enable-testing --scratch-path {{scratch}}/oracle

# Build the original Downright.app from the submodule (for window-level conformance).
downright-app:
    cd vendor/downright && SCRATCH=../../target/downright-app Scripts/bundle-app.sh

# Run the Rust test suite.
test:
    cargo test --workspace

# Regenerate corpus/generated from the pinned submodules.
corpus:
    python3 scripts/build-corpus.py

# Compare Upleft with Downright on the corpus. Pass suite filters through, e.g. `just conform --suite parse`.
conform *args: corpus
    cargo build --release -p upleft-conformance
    target/release/conform {{args}}
