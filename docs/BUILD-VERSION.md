# Build version

AppKit changes behaviour depending on the SDK a program was linked against. These are its linked-on-or-after compatibility switches, and they read the SDK version in the main executable's `LC_BUILD_VERSION` load command. One visible example: a SwiftPM build of the Swift oracle records `sdk 14.0` and gets legacy scrollers, while the same code stamped `sdk 27.0` gets overlay scrollers. Two binaries can only be compared pixel for pixel when they carry the same build version.

**Canonical stamp:** every Swift reference binary and every Upleft binary carries

- `minos 14.0`, Downright's deployment target from its `Package.swift`, and
- `sdk` set to the installed macOS SDK (`xcrun --sdk macosx --show-sdk-version`).

An Xcode build of Downright, which is how its releases are produced, records the real SDK. The `sdk 14.0` that SwiftPM writes is an artifact of the build tool, not of Downright.

**How each side gets it:**

- Rust sets `MACOSX_DEPLOYMENT_TARGET=14.0` in `.cargo/config.toml`, and the linker records the SDK it links against.
- The Swift oracle and the reference `Downright.app` are re-stamped by `just stamp` using `vtool -set-build-version macos 14.0 <sdk> -replace`, then re-signed ad hoc. `just oracle` and `just downright-app` do this automatically.

Check any binary with `vtool -show-build <binary>`.
