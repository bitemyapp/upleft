//! Links Sparkle.framework (2.9.6) into the app binary only, as Downright's
//! Package.swift links Sparkle into the host app and nothing else. The
//! framework is SwiftPM's resolved artifact (`scripts/sparkle-framework.sh`);
//! `UPLEFT_SPARKLE_FRAMEWORK_DIR` names the directory that holds
//! `Sparkle.framework`. Without it the app builds with no updater, as
//! Downright's bundle script warns ("bundle has no updater").

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=UPLEFT_SPARKLE_FRAMEWORK_DIR");
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let default = manifest
        .join("../../target/app-oracle/artifacts/sparkle/Sparkle/Sparkle.xcframework/macos-arm64_x86_64");
    let directory = std::env::var_os("UPLEFT_SPARKLE_FRAMEWORK_DIR").map(PathBuf::from).unwrap_or(default);
    if !directory.join("Sparkle.framework").is_dir() {
        println!("cargo:warning=Sparkle.framework not found under {}; the app is built without an updater", directory.display());
        return;
    }
    let directory = directory.canonicalize().unwrap_or(directory);
    println!("cargo:rerun-if-changed={}", directory.join("Sparkle.framework").display());
    println!("cargo:rustc-link-search=framework={}", directory.display());
    println!("cargo:rustc-link-lib=framework=Sparkle");
    // Inside the bundle; and, for a run straight out of target/, the build
    // directory (a stale entry in a relocated bundle is harmless, as in
    // Downright's bundle script).
    println!("cargo:rustc-link-arg-bins=-Wl,-rpath,@executable_path/../Frameworks");
    println!("cargo:rustc-link-arg-bins=-Wl,-rpath,{}", directory.display());
}
