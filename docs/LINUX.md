# Upleft on Linux

On Linux, Upleft runs through [Sidestep](https://github.com/bitemyapp/sidestep),
which supplies the Objective-C runtime, Foundation and AppKit that objc2 binds.
The code is Upleft's own, with the Linux adaptations committed on this branch;
it builds as an ordinary Cargo workspace, with no wrapper or shell.

## Once

1. Clone Sidestep beside this repository, as `../sidestep`, on a branch with
   the NixOS run-time discovery (currently `native-toolkit`). The
   workspace names it by that path.
2. Generate Sidestep's objc2 overlay, the objc2 crates with Sidestep's fixes
   that `[patch.crates-io]` points at. Run it again after updating Sidestep:

   ```sh
   cargo run --release --manifest-path ../sidestep/tools/objc2-overlay/Cargo.toml
   ```

3. Fetch the reference libraries the build and tests read:

   ```sh
   git submodule update --init
   ```

A C compiler builds `vendor/swift-cmark`. Nothing else is needed: Sidestep
loads fontconfig and the keyboard's data at run time, and finds them on NixOS
as on other distributions.

## Build, run and test

```sh
cargo run -p upleft -- path/to/document.md
```

Run the tests on Sidestep's null backend, which draws windows off screen, so
they don't depend on your session (the results below were measured so):

```sh
SIDESTEP_BACKEND=null cargo test -p upleft-app -p upleft
```

Every test binary links the whole stack, and with full debug info the app's
and libraries' tests take about 100 GB in `target/`; without it
(`CARGO_PROFILE_DEV_DEBUG=0`) about 15 GB.

The libraries test the same way (`-p upleft-core -p upleft-render
-p upleft-markup -p upleft-swift-text -p upleft-mermaid -p upleft-elk
-p upleft-math -p upleft-foundation -p upleft-cli -p upleft-spotlight-metadata`).
Name packages rather than building the whole workspace: `upleft-conformance`,
`upleft-quicklook`, `upleft-thumb` and `upleft-spotlight-importer` are macOS
only.

## What differs

With the packages above, 1838 of 1852 tests pass. The 14 that fail:

- A file URL's path is composed (NFC) here and decomposed (NFD) on macOS
  (five tests), and App Intents need Apple's SDK (one).
- Sentences split by Unicode's rules, which know no abbreviations: "Dr. Smith"
  ends a sentence after "Dr." (two).
- Four math typesetting positions, rendered-link hit testing, and the
  streamed-message comparison, which differs only in the path token value's
  description (its address).

There's no Sparkle, so no automatic updates.
