# Upleft's day-to-day tasks. `just` alone lists them.

scratch := justfile_directory() / "target"

default:
    @just --list --unsorted

# Build downright-oracle, the Swift reference, from vendor/downright.
oracle:
    cd oracle && swift build -c release --scratch-path {{scratch}}/oracle

# Build the original Downright.app from the submodule (for window-level conformance).
downright-app:
    cd vendor/downright && SCRATCH=../../target/downright-app Scripts/bundle-app.sh

# Run the Rust test suite.
test:
    cargo test --workspace
